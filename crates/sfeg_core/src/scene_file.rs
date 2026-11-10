//! Scene files: scenes written as data (ADR 0011, 0014, 0026).
//!
//! A scene file is RON. It names its scene state and each object's type by
//! the name the type was given with `#[sfeg::scene_file_state(...)]` or
//! `#[sfeg::scene_file_object(...)]`, nests children inside their parent,
//! and puts each type's own fields under `data`:
//!
//! ```ron
//! (
//!     scene_state: (type: "Level", data: (level: Meadow)),
//!     active_camera: (
//!         object: Object("camera"),
//!         projection: Perspective(vertical_fov_radians: 1.0472, near: 0.1, far: 100.0),
//!     ),
//!     objects: [
//!         (type: "Walker", id: "walker", data: (speed: 3.0), children: [
//!             (type: "Hat", transform: (position: (x: 0.0, y: 0.75, z: 0.0))),
//!         ]),
//!         (type: "Door", data: (walker: Object("walker"))),
//!         (type: "Tree", data: (texture: Asset("tree"))),
//!     ],
//! )
//! ```
//!
//! - `scene_state` is optional. Without it, the scene state is `()`.
//! - `id` is only needed on objects something refers to. IDs are unique
//!   within one file and are gone once it has loaded.
//! - A [`Handle`] field is written `Object("id")`, and a [`TextureHandle`]
//!   field `Asset("name")`, naming an asset in the manifest. Both markers
//!   are reserved: the engine and the build find every reference by them,
//!   without knowing the game's types.
//! - A missing `transform` is the identity, and a missing `data` is `()`.
//! - `Some(...)` may be left out around optional values, and an enum
//!   variant holding a struct may be written `Variant(field: ...)`.

use std::any::{TypeId, type_name};
use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use ron::extensions::Extensions;
use ron::value::RawValue;
use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer};

use crate::assets::{AssetManifest, AssetScope, AssetStore, ManifestAsset, TextureHandle};
use crate::camera::Projection;
use crate::game_object::GameObject;
use crate::handle::{Handle, ObjectHandle};
use crate::math::Transform;
use crate::scene::{Parent, Scene};
use crate::scene_state::SceneState;

/// A game object type that scene files can name (ADR 0014, 0026).
///
/// Implemented by `#[sfeg::scene_file_object(name = "...")]`, which also
/// derives `Deserialize` for the type's fields. Register the type in a
/// [`SceneTypeRegistry`] for files to use it.
pub trait SceneFileObject: GameObject + DeserializeOwned {
    /// The name scene files use for the type. Never taken from the Rust
    /// type name, so renaming the type never breaks files.
    const NAME: &'static str;
}

/// A scene state type that scene files can name (ADR 0014, 0026).
///
/// Implemented by `#[sfeg::scene_file_state(name = "...")]`, which also
/// derives `Deserialize` for the type's fields. Register the type in a
/// [`SceneTypeRegistry`] for files to use it.
pub trait SceneFileState: SceneState + DeserializeOwned {
    /// The name scene files use for the type. Never taken from the Rust
    /// type name, so renaming the type never breaks files.
    const NAME: &'static str;
}

/// The types scene files can name, passed to `CoreSettings::scene_types`
/// (ADR 0014).
///
/// Every type is registered with an explicit call. Object and scene state
/// types share one set of names.
#[derive(Debug, Clone, Default)]
pub struct SceneTypeRegistry {
    types: HashMap<&'static str, RegisteredType>,
}

#[derive(Debug, Clone, Copy)]
enum RegisteredType {
    Object {
        type_id: TypeId,
        deserialize: fn(&str) -> Result<Box<dyn GameObject>, ron::error::SpannedError>,
    },
    SceneState {
        deserialize: fn(&str) -> Result<Box<dyn SceneState>, ron::error::SpannedError>,
    },
}

impl SceneTypeRegistry {
    /// A registry with no types, for a game that loads no scene files or
    /// registers its types next.
    pub fn new() -> SceneTypeRegistry {
        SceneTypeRegistry::default()
    }

    /// Lets scene files spawn `T` by its [`SceneFileObject::NAME`].
    ///
    /// # Errors
    ///
    /// [`RegisterSceneTypeError::NameTaken`] if a type is already
    /// registered under that name, including `T` itself. Nothing changes.
    pub fn register_object<T: SceneFileObject>(&mut self) -> Result<(), RegisterSceneTypeError> {
        self.register(
            T::NAME,
            RegisteredType::Object {
                type_id: TypeId::of::<T>(),
                deserialize: |ron| {
                    let object: T = scene_file_options().from_str(ron)?;
                    Ok(Box::new(object))
                },
            },
        )
    }

    /// Lets scene files use `S` as their scene state by its
    /// [`SceneFileState::NAME`].
    ///
    /// # Errors
    ///
    /// [`RegisterSceneTypeError::NameTaken`] if a type is already
    /// registered under that name, including `S` itself. Nothing changes.
    pub fn register_scene_state<S: SceneFileState>(
        &mut self,
    ) -> Result<(), RegisterSceneTypeError> {
        self.register(
            S::NAME,
            RegisteredType::SceneState {
                deserialize: |ron| {
                    let scene_state: S = scene_file_options().from_str(ron)?;
                    Ok(Box::new(scene_state))
                },
            },
        )
    }

    fn register(
        &mut self,
        name: &'static str,
        registered: RegisteredType,
    ) -> Result<(), RegisterSceneTypeError> {
        if self.types.contains_key(name) {
            return Err(RegisterSceneTypeError::NameTaken { name });
        }
        self.types.insert(name, registered);
        Ok(())
    }
}

/// Why a type couldn't be registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegisterSceneTypeError {
    /// Another type, or the same one, is already registered under this
    /// name.
    NameTaken {
        /// The name both types use.
        name: &'static str,
    },
}

impl fmt::Display for RegisterSceneTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegisterSceneTypeError::NameTaken { name } => {
                write!(f, "a type is already registered as {name:?}")
            }
        }
    }
}

impl Error for RegisterSceneTypeError {}

/// Why a scene file couldn't be loaded or built.
#[derive(Debug)]
#[non_exhaustive]
pub enum SceneFileError {
    /// The file couldn't be read.
    ReadFile {
        /// Where the file was looked for.
        path: PathBuf,
        /// Why reading failed.
        source: std::io::Error,
    },
    /// The file isn't valid RON, or its engine keys are wrong, such as a
    /// misspelled `children`.
    Parse {
        /// What the parser found, with its line and column.
        source: Box<dyn Error + Send + Sync>,
    },
    /// An `Asset(...)` or `Object(...)` doesn't hold one name in quotes.
    MalformedReference {
        /// `"Asset"` or `"Object"`.
        marker: &'static str,
    },
    /// Two objects in the file have the same ID.
    DuplicateId {
        /// The ID they share.
        id: String,
    },
    /// An `Object("id")` names an ID no object in the file has.
    UnknownObjectId {
        /// The ID that was referred to.
        id: String,
    },
    /// An `Object("id")` names an object whose type isn't the one the
    /// handle field wants.
    WrongObjectType {
        /// The ID that was referred to.
        id: String,
        /// The Rust type the handle field wants.
        expected: &'static str,
    },
    /// An `Asset("name")` names nothing in the asset manifest.
    UnknownAssetName {
        /// The name that was referred to.
        name: String,
    },
    /// An `Asset("name")` names an asset of another kind than the field
    /// wants, such as a scene file in a texture field.
    WrongAssetKind {
        /// The name that was referred to.
        name: String,
        /// The kind the field wants, such as `"Texture"`.
        expected: &'static str,
        /// The kind the manifest gives it.
        found: &'static str,
    },
    /// A `type` names nothing registered in the [`SceneTypeRegistry`].
    UnknownTypeName {
        /// The name in the file.
        type_name: String,
    },
    /// An object's `type` names a registered scene state type.
    NotAnObjectType {
        /// The name in the file.
        type_name: String,
    },
    /// The scene state's `type` names a registered object type.
    NotASceneStateType {
        /// The name in the file.
        type_name: String,
    },
    /// An object's `data` doesn't match its type's fields.
    ObjectData {
        /// The object's type name in the file.
        type_name: String,
        /// The object's ID, if it has one.
        id: Option<String>,
        /// What didn't match. Its line and column count from the start of
        /// `data`.
        source: Box<dyn Error + Send + Sync>,
    },
    /// The scene state's `data` doesn't match its type's fields.
    SceneStateData {
        /// The scene state's type name in the file.
        type_name: String,
        /// What didn't match. Its line and column count from the start of
        /// `data`.
        source: Box<dyn Error + Send + Sync>,
    },
    /// A handle or asset field was read outside a scene file load, such as
    /// from a config file. Those references only mean something inside the
    /// file they are written in.
    ReadOutsideSceneFileLoad,
    /// No loader thread could read the file.
    LoaderUnavailable,
}

impl fmt::Display for SceneFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SceneFileError::ReadFile { path, .. } => {
                write!(f, "couldn't read the scene file {}", path.display())
            }
            SceneFileError::Parse { .. } => write!(f, "the scene file isn't valid"),
            SceneFileError::MalformedReference { marker } => {
                write!(f, "a {marker}(...) must hold exactly one name in quotes")
            }
            SceneFileError::DuplicateId { id } => {
                write!(f, "two objects have the ID {id:?}")
            }
            SceneFileError::UnknownObjectId { id } => {
                write!(f, "Object({id:?}) names an ID no object in the file has")
            }
            SceneFileError::WrongObjectType { id, expected } => {
                write!(f, "Object({id:?}) isn't a {expected}")
            }
            SceneFileError::UnknownAssetName { name } => {
                write!(f, "Asset({name:?}) isn't in the asset manifest")
            }
            SceneFileError::WrongAssetKind {
                name,
                expected,
                found,
            } => write!(
                f,
                "Asset({name:?}) is a {found}, but the field wants a {expected}"
            ),
            SceneFileError::UnknownTypeName { type_name } => {
                write!(f, "no type is registered as {type_name:?}")
            }
            SceneFileError::NotAnObjectType { type_name } => {
                write!(f, "{type_name:?} is a scene state type, not an object type")
            }
            SceneFileError::NotASceneStateType { type_name } => {
                write!(f, "{type_name:?} is an object type, not a scene state type")
            }
            SceneFileError::ObjectData { type_name, id, .. } => match id {
                Some(id) => write!(
                    f,
                    "the data of the {type_name} {id:?} doesn't match its type"
                ),
                None => write!(f, "the data of a {type_name} doesn't match its type"),
            },
            SceneFileError::SceneStateData { type_name, .. } => {
                write!(
                    f,
                    "the data of the scene state {type_name} doesn't match its type"
                )
            }
            SceneFileError::ReadOutsideSceneFileLoad => write!(
                f,
                "Object(...) and Asset(...) can only be read while a scene file loads"
            ),
            SceneFileError::LoaderUnavailable => {
                write!(f, "no asset loader thread could read the scene file")
            }
        }
    }
}

impl Error for SceneFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            SceneFileError::ReadFile { source, .. } => Some(source),
            SceneFileError::Parse { source }
            | SceneFileError::ObjectData { source, .. }
            | SceneFileError::SceneStateData { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

/// The references a scene file makes, found without knowing the game's
/// types. The build uses them to check a game's scene files (ADR 0026).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneFileReferences {
    /// Every name in an `Asset("name")`, in file order, once per
    /// reference.
    pub asset_names: Vec<String>,
}

/// Parses `text` as a scene file and checks what can be checked without
/// the game's types: its engine keys, that IDs are unique, and that every
/// `Object("id")` names one of them. Returns the assets it refers to.
///
/// `sfeg_build` calls this for every scene file in a game's manifest.
///
/// # Errors
///
/// [`SceneFileError::Parse`], [`SceneFileError::MalformedReference`],
/// [`SceneFileError::DuplicateId`], or [`SceneFileError::UnknownObjectId`].
pub fn check_scene_file_text(text: &str) -> Result<SceneFileReferences, SceneFileError> {
    let tree = SceneFileTree::parse(text)?;
    Ok(SceneFileReferences {
        asset_names: tree.asset_names,
    })
}

/// A parsed scene file whose types haven't been looked up yet. Parsing
/// needs no game types, so it happens on a loader thread.
#[derive(Debug)]
pub(crate) struct SceneFileTree {
    layout: SceneFileLayout,
    /// Every name in an `Asset("name")`, in file order.
    asset_names: Vec<String>,
}

/// The engine keys of a scene file. Each type's own fields stay unparsed
/// until its type is known.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneFileLayout {
    #[serde(default)]
    scene_state: Option<SceneStateEntry>,
    #[serde(default)]
    active_camera: Option<ActiveCameraEntry>,
    #[serde(default)]
    objects: Vec<ObjectEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneStateEntry {
    #[serde(rename = "type")]
    type_name: String,
    #[serde(default)]
    data: Option<Box<RawValue>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveCameraEntry {
    object: ObjectReference,
    projection: Projection,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectEntry {
    #[serde(rename = "type")]
    type_name: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    transform: Option<Transform>,
    #[serde(default)]
    data: Option<Box<RawValue>>,
    #[serde(default)]
    children: Vec<ObjectEntry>,
}

/// `Object("id")` in a file.
#[derive(Debug, Deserialize)]
#[serde(rename = "Object")]
struct ObjectReference(String);

/// `Asset("name")` in a file.
#[derive(Debug, Deserialize)]
#[serde(rename = "Asset")]
struct AssetReference(String);

impl SceneFileTree {
    /// Reads and parses the file at `path`.
    pub(crate) fn read(path: &Path) -> Result<SceneFileTree, SceneFileError> {
        let text = std::fs::read_to_string(path).map_err(|source| SceneFileError::ReadFile {
            path: path.to_path_buf(),
            source,
        })?;
        SceneFileTree::parse(&text)
    }

    fn parse(text: &str) -> Result<SceneFileTree, SceneFileError> {
        let layout: SceneFileLayout =
            scene_file_options()
                .from_str(text)
                .map_err(|source| SceneFileError::Parse {
                    source: Box::new(source),
                })?;
        let references = scan_references(text)?;

        let mut ids = Vec::new();
        collect_ids(&layout.objects, &mut ids);
        let mut seen: Vec<&str> = Vec::with_capacity(ids.len());
        for id in &ids {
            if seen.contains(id) {
                return Err(SceneFileError::DuplicateId { id: id.to_string() });
            }
            seen.push(id);
        }
        if let Some(id) = references
            .object_ids
            .iter()
            .find(|id| !seen.contains(&id.as_str()))
        {
            return Err(SceneFileError::UnknownObjectId { id: id.clone() });
        }
        Ok(SceneFileTree {
            layout,
            asset_names: references.asset_names,
        })
    }
}

fn collect_ids<'a>(objects: &'a [ObjectEntry], ids: &mut Vec<&'a str>) {
    for object in objects {
        if let Some(id) = &object.id {
            ids.push(id);
        }
        collect_ids(&object.children, ids);
    }
}

/// How scene files are parsed: `Some` may be left out, and a variant
/// holding a struct may be written `Variant(field: ...)`.
fn scene_file_options() -> ron::Options {
    ron::Options::default()
        .with_default_extension(Extensions::IMPLICIT_SOME | Extensions::UNWRAP_VARIANT_NEWTYPES)
}

/// Every `Asset("...")` and `Object("...")` in a file, in file order.
struct ScannedReferences {
    asset_names: Vec<String>,
    object_ids: Vec<String>,
}

/// Finds every reference marker in `text`, skipping comments and strings,
/// so the build can check references without knowing the game's types.
fn scan_references(text: &str) -> Result<ScannedReferences, SceneFileError> {
    let mut found = ScannedReferences {
        asset_names: Vec::new(),
        object_ids: Vec::new(),
    };
    let mut scanner = Scanner { text, at: 0 };
    while let Some(byte) = scanner.peek() {
        match byte {
            b'/' if scanner.starts_with("//") || scanner.starts_with("/*") => {
                scanner.skip_comment();
            }
            b'"' => {
                scanner.read_string();
            }
            b'\'' => scanner.skip_char_literal(),
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let ident = scanner.read_ident();
                if (ident == "r" || ident == "br") && matches!(scanner.peek(), Some(b'#' | b'"')) {
                    scanner.skip_raw_string();
                    continue;
                }
                let marker = match ident {
                    "Asset" => "Asset",
                    "Object" => "Object",
                    _ => continue,
                };
                let name = scanner
                    .read_reference_argument()
                    .ok_or(SceneFileError::MalformedReference { marker })?;
                match marker {
                    "Asset" => found.asset_names.push(name),
                    _ => found.object_ids.push(name),
                }
            }
            _ => scanner.at += 1,
        }
    }
    Ok(found)
}

/// Walks a RON text one token at a time, far enough to find references.
struct Scanner<'a> {
    text: &'a str,
    /// A byte offset into `text`.
    at: usize,
}

impl<'a> Scanner<'a> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn starts_with(&self, prefix: &str) -> bool {
        self.text.as_bytes()[self.at..].starts_with(prefix.as_bytes())
    }

    /// Skips a `//` line comment or a `/* */` block comment, which may
    /// nest.
    fn skip_comment(&mut self) {
        if self.starts_with("//") {
            while let Some(byte) = self.peek() {
                self.at += 1;
                if byte == b'\n' {
                    return;
                }
            }
            return;
        }
        let mut depth = 0usize;
        while self.at < self.text.len() {
            if self.starts_with("/*") {
                depth += 1;
                self.at += 2;
            } else if self.starts_with("*/") {
                depth -= 1;
                self.at += 2;
                if depth == 0 {
                    return;
                }
            } else {
                self.at += 1;
            }
        }
    }

    /// Reads a quoted string starting at `"`, and returns it with its
    /// quotes, as written. Stops at the end of the text if it never
    /// closes.
    fn read_string(&mut self) -> &'a str {
        let start = self.at;
        self.at += 1;
        while let Some(byte) = self.peek() {
            self.at += 1;
            match byte {
                b'\\' => self.at += 1,
                b'"' => break,
                _ => {}
            }
        }
        self.at = self.at.min(self.text.len());
        &self.text[start..self.at]
    }

    /// Skips the rest of a raw string, from its `#`s or opening `"`.
    fn skip_raw_string(&mut self) {
        let mut hashes = 0;
        while self.peek() == Some(b'#') {
            hashes += 1;
            self.at += 1;
        }
        self.at += 1;
        let closing = format!("\"{}", "#".repeat(hashes));
        match self.text[self.at.min(self.text.len())..].find(&closing) {
            Some(offset) => self.at += offset + closing.len(),
            None => self.at = self.text.len(),
        }
    }

    /// Skips a character literal such as `'a'` or `'\''`, so a quote
    /// inside one isn't taken as the start of a string.
    fn skip_char_literal(&mut self) {
        self.at += 1;
        if self.peek() == Some(b'\\') {
            self.at += 1;
        }
        // Past one whole character, which may be several bytes.
        if let Some(c) = self.text[self.at.min(self.text.len())..].chars().next() {
            self.at += c.len_utf8();
        }
        while let Some(byte) = self.peek() {
            self.at += 1;
            if byte == b'\'' {
                return;
            }
        }
    }

    fn read_ident(&mut self) -> &'a str {
        let start = self.at;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_alphanumeric() || byte == b'_' {
                self.at += 1;
            } else {
                break;
            }
        }
        &self.text[start..self.at]
    }

    fn skip_whitespace_and_comments(&mut self) {
        while let Some(byte) = self.peek() {
            if byte.is_ascii_whitespace() {
                self.at += 1;
            } else if self.starts_with("//") || self.starts_with("/*") {
                self.skip_comment();
            } else {
                return;
            }
        }
    }

    /// Reads `("name")` after a reference marker. `None` if it isn't
    /// exactly that.
    fn read_reference_argument(&mut self) -> Option<String> {
        self.skip_whitespace_and_comments();
        if self.peek() != Some(b'(') {
            return None;
        }
        self.at += 1;
        self.skip_whitespace_and_comments();
        if self.peek() != Some(b'"') {
            return None;
        }
        let quoted = self.read_string();
        let name: String = ron::from_str(quoted).ok()?;
        self.skip_whitespace_and_comments();
        if self.peek() != Some(b')') {
            return None;
        }
        self.at += 1;
        Some(name)
    }
}

/// What handle and asset fields read while a scene file is built
/// (ADR 0026).
struct SceneFileLoad {
    objects_by_id: HashMap<String, ReservedObject>,
    textures_by_name: HashMap<String, TextureHandle>,
    manifest: AssetManifest,
    /// The first reference that failed, so the build can return it typed
    /// instead of as the message serde carried.
    first_error: Option<SceneFileError>,
}

#[derive(Debug, Clone, Copy)]
struct ReservedObject {
    handle: ObjectHandle,
    type_id: TypeId,
}

thread_local! {
    /// Set only while [`build_scene`] runs on this thread.
    static SCENE_FILE_LOAD: RefCell<Option<SceneFileLoad>> = const { RefCell::new(None) };
}

/// Clears the load when the build ends, even by an early return.
struct SceneFileLoadGuard;

impl SceneFileLoadGuard {
    fn begin(load: SceneFileLoad) -> SceneFileLoadGuard {
        SCENE_FILE_LOAD.with(|current| {
            let previous = current.borrow_mut().replace(load);
            assert!(previous.is_none(), "scene file builds don't nest");
        });
        SceneFileLoadGuard
    }

    /// Takes the first reference error recorded since the last call.
    fn take_first_error(&self) -> Option<SceneFileError> {
        SCENE_FILE_LOAD.with(|current| {
            current
                .borrow_mut()
                .as_mut()
                .and_then(|load| load.first_error.take())
        })
    }
}

impl Drop for SceneFileLoadGuard {
    fn drop(&mut self) {
        SCENE_FILE_LOAD.with(|current| current.borrow_mut().take());
    }
}

/// Runs `resolve` against the load in progress, recording its error.
fn with_scene_file_load<R>(
    resolve: impl FnOnce(&SceneFileLoad) -> Result<R, SceneFileError>,
) -> Result<R, String> {
    SCENE_FILE_LOAD.with(|current| {
        let mut current = current.borrow_mut();
        let Some(load) = current.as_mut() else {
            return Err(SceneFileError::ReadOutsideSceneFileLoad.to_string());
        };
        resolve(load).map_err(|error| {
            let message = error.to_string();
            load.first_error.get_or_insert(error);
            message
        })
    })
}

impl<'de, T: GameObject> Deserialize<'de> for Handle<T> {
    /// Reads `Object("id")` while a scene file loads (ADR 0026).
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Handle<T>, D::Error> {
        let ObjectReference(id) = ObjectReference::deserialize(deserializer)?;
        with_scene_file_load(|load| {
            let reserved = load
                .objects_by_id
                .get(&id)
                .ok_or_else(|| SceneFileError::UnknownObjectId { id: id.clone() })?;
            if reserved.type_id != TypeId::of::<T>() {
                return Err(SceneFileError::WrongObjectType {
                    id: id.clone(),
                    expected: type_name::<T>(),
                });
            }
            Ok(Handle::new(reserved.handle))
        })
        .map_err(D::Error::custom)
    }
}

impl<'de> Deserialize<'de> for ObjectHandle {
    /// Reads `Object("id")` while a scene file loads (ADR 0026).
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<ObjectHandle, D::Error> {
        let ObjectReference(id) = ObjectReference::deserialize(deserializer)?;
        with_scene_file_load(|load| {
            load.objects_by_id
                .get(&id)
                .map(|reserved| reserved.handle)
                .ok_or_else(|| SceneFileError::UnknownObjectId { id: id.clone() })
        })
        .map_err(D::Error::custom)
    }
}

impl<'de> Deserialize<'de> for TextureHandle {
    /// Reads `Asset("name")` while a scene file loads (ADR 0026). The
    /// texture was loaded into the file's scope before any object was
    /// read.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<TextureHandle, D::Error> {
        let AssetReference(name) = AssetReference::deserialize(deserializer)?;
        with_scene_file_load(|load| {
            if let Some(texture) = load.textures_by_name.get(&name) {
                return Ok(*texture);
            }
            match load.manifest.asset_named(&name) {
                None => Err(SceneFileError::UnknownAssetName { name: name.clone() }),
                Some(other) => Err(SceneFileError::WrongAssetKind {
                    name: name.clone(),
                    expected: "Texture",
                    found: other.kind_name(),
                }),
            }
        })
        .map_err(D::Error::custom)
    }
}

/// Builds a scene from `tree`, loading every asset it refers to into
/// `scope` (ADR 0026).
///
/// Objects spawn in file order, parents before their children, so each
/// object's handle is known before any is read and `Object("id")` can
/// refer forward as well as back.
pub(crate) fn build_scene(
    tree: SceneFileTree,
    scene_types: &SceneTypeRegistry,
    manifest: &AssetManifest,
    assets: &mut AssetStore,
    scope: AssetScope,
) -> Result<Scene, SceneFileError> {
    let SceneFileTree {
        layout,
        asset_names,
    } = tree;

    // A new scene fills slots from zero, so the handles are known now.
    let mut objects_by_id = HashMap::new();
    let mut next_index = 0;
    reserve_handles(
        &layout.objects,
        scene_types,
        &mut next_index,
        &mut objects_by_id,
    )?;

    let mut texture_assets = Vec::new();
    for name in &asset_names {
        match manifest.asset_named(name) {
            None => return Err(SceneFileError::UnknownAssetName { name: name.clone() }),
            Some(ManifestAsset::Texture(texture)) => texture_assets.push((name, texture)),
            // Not loaded here. A field reading it reports the wrong kind.
            Some(_) => {}
        }
    }
    // Every reference is one load, so the scene's count matches the file.
    let mut textures_by_name = HashMap::new();
    for (name, texture) in texture_assets {
        textures_by_name.insert(name.clone(), assets.load_texture(texture, scope));
    }

    let guard = SceneFileLoadGuard::begin(SceneFileLoad {
        objects_by_id,
        textures_by_name,
        manifest: *manifest,
        first_error: None,
    });

    let mut scene = match &layout.scene_state {
        None => Scene::new(()),
        Some(entry) => {
            let Some(registered) = scene_types.types.get(entry.type_name.as_str()) else {
                return Err(SceneFileError::UnknownTypeName {
                    type_name: entry.type_name.clone(),
                });
            };
            let RegisteredType::SceneState { deserialize } = registered else {
                return Err(SceneFileError::NotASceneStateType {
                    type_name: entry.type_name.clone(),
                });
            };
            let data = entry.data.as_ref().map_or("()", |data| data.get_ron());
            let scene_state = deserialize(data).map_err(|source| {
                guard
                    .take_first_error()
                    .unwrap_or_else(|| SceneFileError::SceneStateData {
                        type_name: entry.type_name.clone(),
                        source: Box::new(source),
                    })
            })?;
            Scene::with_boxed_scene_state(scene_state)
        }
    };

    for object in &layout.objects {
        spawn_entry(object, Parent::Root, scene_types, &mut scene, &guard)?;
    }

    if let Some(camera) = &layout.active_camera {
        let ObjectReference(id) = &camera.object;
        let handle = SCENE_FILE_LOAD.with(|current| {
            current
                .borrow()
                .as_ref()
                .and_then(|load| load.objects_by_id.get(id))
                .map(|reserved| reserved.handle)
        });
        let handle = handle.ok_or_else(|| SceneFileError::UnknownObjectId { id: id.clone() })?;
        scene
            .set_active_camera(handle, camera.projection)
            .expect("every object in the file was spawned");
    }
    drop(guard);
    Ok(scene)
}

/// Gives each object the handle it will spawn with, in spawn order, and
/// checks that each type is a registered object type.
fn reserve_handles(
    objects: &[ObjectEntry],
    scene_types: &SceneTypeRegistry,
    next_index: &mut usize,
    objects_by_id: &mut HashMap<String, ReservedObject>,
) -> Result<(), SceneFileError> {
    for object in objects {
        let type_id = object_type(object, scene_types)?.0;
        let handle = ObjectHandle {
            index: *next_index,
            generation: 0,
        };
        *next_index += 1;
        if let Some(id) = &object.id {
            objects_by_id.insert(id.clone(), ReservedObject { handle, type_id });
        }
        reserve_handles(&object.children, scene_types, next_index, objects_by_id)?;
    }
    Ok(())
}

type DeserializeObject = fn(&str) -> Result<Box<dyn GameObject>, ron::error::SpannedError>;

fn object_type(
    object: &ObjectEntry,
    scene_types: &SceneTypeRegistry,
) -> Result<(TypeId, DeserializeObject), SceneFileError> {
    match scene_types.types.get(object.type_name.as_str()) {
        None => Err(SceneFileError::UnknownTypeName {
            type_name: object.type_name.clone(),
        }),
        Some(RegisteredType::SceneState { .. }) => Err(SceneFileError::NotAnObjectType {
            type_name: object.type_name.clone(),
        }),
        Some(RegisteredType::Object {
            type_id,
            deserialize,
        }) => Ok((*type_id, *deserialize)),
    }
}

fn spawn_entry(
    object: &ObjectEntry,
    parent: Parent,
    scene_types: &SceneTypeRegistry,
    scene: &mut Scene,
    guard: &SceneFileLoadGuard,
) -> Result<(), SceneFileError> {
    let (type_id, deserialize) = object_type(object, scene_types)?;
    let data = object.data.as_ref().map_or("()", |data| data.get_ron());
    let game_object = deserialize(data).map_err(|source| {
        guard
            .take_first_error()
            .unwrap_or_else(|| SceneFileError::ObjectData {
                type_name: object.type_name.clone(),
                id: object.id.clone(),
                source: Box::new(source),
            })
    })?;
    let local = object.transform.unwrap_or(Transform::IDENTITY);
    let handle = scene.spawn_boxed_immediate(game_object, type_id, parent, local);
    for child in &object.children {
        spawn_entry(child, Parent::Object(handle), scene_types, scene, guard)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_are_found_outside_strings_and_comments() {
        let text = r##"(
            // Asset("in_a_comment")
            /* Object("in /* a nested */ block") */
            objects: [
                (type: "A", id: "a", data: (x: "Asset(\"in_a_string\")", y: 'A', t: Asset("tree"))),
                (type: "B", data: (target: Object( "a" ), raw: r#"Object("raw")"#, MyAsset: 1)),
            ],
        )"##;
        let found = scan_references(text).unwrap();
        assert_eq!(found.asset_names, vec!["tree"]);
        assert_eq!(found.object_ids, vec!["a"]);
    }

    #[test]
    fn a_marker_without_a_quoted_name_is_malformed() {
        assert!(matches!(
            scan_references("(t: Asset(tree))"),
            Err(SceneFileError::MalformedReference { marker: "Asset" })
        ));
        assert!(matches!(
            scan_references(r#"(t: Object("a", "b"))"#),
            Err(SceneFileError::MalformedReference { marker: "Object" })
        ));
    }

    #[test]
    fn checking_finds_duplicate_and_unknown_ids() {
        let duplicate = r#"(objects: [(type: "A", id: "a"), (type: "A", id: "a")])"#;
        assert!(matches!(
            check_scene_file_text(duplicate),
            Err(SceneFileError::DuplicateId { id }) if id == "a"
        ));
        let unknown = r#"(objects: [(type: "A", id: "a", data: (t: Object("b")))])"#;
        assert!(matches!(
            check_scene_file_text(unknown),
            Err(SceneFileError::UnknownObjectId { id }) if id == "b"
        ));
        let camera = r#"(
            active_camera: (object: Object("cam"), projection: Orthographic(visible_height: 2.0, near: 0.0, far: 1.0)),
            objects: [(type: "A", id: "camera")],
        )"#;
        assert!(matches!(
            check_scene_file_text(camera),
            Err(SceneFileError::UnknownObjectId { id }) if id == "cam"
        ));
    }

    #[test]
    fn checking_returns_every_asset_reference() {
        let text = r#"(objects: [
            (type: "Tree", data: (texture: Asset("tree"))),
            (type: "Tree", data: (texture: Asset("tree")), children: [
                (type: "Rock", data: (texture: Asset("rock"))),
            ]),
        ])"#;
        assert_eq!(
            check_scene_file_text(text).unwrap().asset_names,
            vec!["tree", "tree", "rock"]
        );
    }

    #[test]
    fn misspelled_engine_keys_fail_to_parse() {
        assert!(matches!(
            check_scene_file_text(r#"(objects: [(type: "A", childen: [])])"#),
            Err(SceneFileError::Parse { .. })
        ));
    }

    use std::path::PathBuf;

    use serde::Deserialize;

    use crate::assets::{AssetManifest, SceneFileAsset, ScopeLoadCounts, TextureAsset};
    use crate::input::InputBindings;
    use crate::scene::TransformInterpolation;
    use crate::time::{MaxTicksPerFrame, TickRate};
    use crate::{Core, CoreSettings, GameState, GameStateTickContext, QueueSceneChangeError};

    #[derive(Deserialize)]
    struct Walker {
        speed: f32,
        #[serde(skip)]
        steps_taken: u32,
    }
    impl GameObject for Walker {}
    impl SceneFileObject for Walker {
        const NAME: &'static str = "Walker";
    }

    #[derive(Deserialize)]
    struct Hat;
    impl GameObject for Hat {}
    impl SceneFileObject for Hat {
        const NAME: &'static str = "Hat";
    }

    #[derive(Deserialize)]
    struct Door {
        walker: Handle<Walker>,
    }
    impl GameObject for Door {}
    impl SceneFileObject for Door {
        const NAME: &'static str = "Door";
    }

    #[derive(Deserialize)]
    struct Tree {
        texture: TextureHandle,
    }
    impl GameObject for Tree {}
    impl SceneFileObject for Tree {
        const NAME: &'static str = "Tree";
    }

    #[derive(Deserialize)]
    struct Level {
        number: u32,
        player: Handle<Walker>,
    }
    impl SceneState for Level {}
    impl SceneFileState for Level {
        const NAME: &'static str = "Level";
    }

    const TREE: TextureAsset = TextureAsset::from_manifest("tree", "tree.png");
    const LEVEL_ONE: SceneFileAsset = SceneFileAsset::from_manifest("level_one", "one.ron");
    const LEVEL_TWO: SceneFileAsset = SceneFileAsset::from_manifest("level_two", "two.ron");
    const MISSING: SceneFileAsset = SceneFileAsset::from_manifest("missing", "missing.ron");
    const MANIFEST: AssetManifest =
        AssetManifest::from_manifest(&[TREE], &[LEVEL_ONE, LEVEL_TWO, MISSING]);

    fn scene_types() -> SceneTypeRegistry {
        let mut scene_types = SceneTypeRegistry::new();
        scene_types.register_object::<Walker>().unwrap();
        scene_types.register_object::<Hat>().unwrap();
        scene_types.register_object::<Door>().unwrap();
        scene_types.register_object::<Tree>().unwrap();
        scene_types.register_scene_state::<Level>().unwrap();
        scene_types
    }

    /// A fresh folder holding `files`, as (name, contents).
    fn folder_with(test_name: &str, files: &[(&str, &str)]) -> PathBuf {
        let folder = std::env::temp_dir()
            .join("sfeg_core_scene_file_tests")
            .join(format!("{test_name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        for (name, contents) in files {
            std::fs::write(folder.join(name), contents).unwrap();
        }
        folder
    }

    fn settings(asset_folder: PathBuf) -> CoreSettings {
        CoreSettings {
            tick_rate: TickRate::DEFAULT,
            max_ticks_per_frame: MaxTicksPerFrame::DEFAULT,
            transform_interpolation: TransformInterpolation::Enabled,
            input_bindings: InputBindings::none(),
            asset_folder,
            asset_manifest: MANIFEST,
            scene_types: scene_types(),
        }
    }

    const LEVEL_ONE_TEXT: &str = r#"(
        scene_state: (type: "Level", data: (number: 1, player: Object("walker"))),
        active_camera: (
            object: Object("camera"),
            projection: Perspective(vertical_fov_radians: 1.0, near: 0.1, far: 100.0),
        ),
        objects: [
            // Refers forward, to an object later in the file.
            (type: "Door", data: (walker: Object("walker"))),
            (type: "Walker", id: "walker", data: (speed: 3.0), children: [
                (type: "Hat", transform: (position: (x: 0.0, y: 0.75, z: 0.0))),
            ]),
            (type: "Tree", data: (texture: Asset("tree"))),
            (type: "Tree", data: (texture: Asset("tree"))),
            (type: "Hat", id: "camera"),
        ],
    )"#;

    fn first_scene_error(test_name: &str, text: &str) -> SceneFileError {
        let folder = folder_with(test_name, &[("one.ron", text)]);
        match Core::new_with_first_scene_file((), LEVEL_ONE, settings(folder)) {
            Ok(_) => panic!("the file loaded"),
            Err(error) => error,
        }
    }

    #[test]
    fn a_first_scene_file_builds_its_objects_scene_state_and_camera() {
        let folder = folder_with("first_scene", &[("one.ron", LEVEL_ONE_TEXT)]);
        let mut core = Core::new_with_first_scene_file((), LEVEL_ONE, settings(folder)).unwrap();
        let scene = core.current_scene();
        assert_eq!(scene.len(), 6);

        let level = scene.scene_state::<Level>().unwrap();
        assert_eq!(level.number, 1);
        let walker = scene.get(level.player).unwrap();
        assert_eq!(walker.speed, 3.0);
        assert_eq!(walker.steps_taken, 0);

        // The hat is the walker's child, with the rest of its transform
        // defaulted.
        let hat = scene.children_of(level.player).unwrap()[0];
        assert_eq!(
            scene.local_of(hat).unwrap().position(),
            crate::math::Vector3::new(0.0, 0.75, 0.0)
        );
        assert_eq!(
            scene.local_of(hat).unwrap().scale(),
            crate::math::Vector3::ONE
        );
        assert_eq!(scene.local_of(level.player), Some(&Transform::IDENTITY));

        let door = scene
            .typed::<Door>(ObjectHandle {
                index: 0,
                generation: 0,
            })
            .unwrap();
        assert_eq!(scene.get(door).unwrap().walker, level.player);
        assert!(core.draw().camera().is_some());

        // The scene holds one load per reference: two trees, two loads.
        let tree = core.load_texture_into_current_scene_scope(TREE);
        let trees: Vec<TextureHandle> = (3..5)
            .map(|index| {
                let handle = core
                    .current_scene()
                    .typed::<Tree>(ObjectHandle {
                        index,
                        generation: 0,
                    })
                    .unwrap();
                core.current_scene().get(handle).unwrap().texture
            })
            .collect();
        assert_eq!(trees, vec![tree, tree]);
    }

    #[test]
    fn a_handle_to_an_object_of_another_type_is_a_typed_error() {
        let text = r#"(objects: [
            (type: "Hat", id: "hat"),
            (type: "Door", data: (walker: Object("hat"))),
        ])"#;
        match first_scene_error("wrong_object_type", text) {
            SceneFileError::WrongObjectType { id, expected } => {
                assert_eq!(id, "hat");
                assert!(expected.ends_with("Walker"), "{expected}");
            }
            error => panic!("{error:?}"),
        }
    }

    #[test]
    fn unknown_and_misused_names_are_typed_errors() {
        let error = first_scene_error("unknown_type", r#"(objects: [(type: "Goblin")])"#);
        assert!(
            matches!(error, SceneFileError::UnknownTypeName { type_name } if type_name == "Goblin")
        );

        let error = first_scene_error("state_as_object", r#"(objects: [(type: "Level")])"#);
        assert!(matches!(error, SceneFileError::NotAnObjectType { .. }));

        let error = first_scene_error("object_as_state", r#"(scene_state: (type: "Hat"))"#);
        assert!(matches!(error, SceneFileError::NotASceneStateType { .. }));

        let error = first_scene_error(
            "unknown_asset",
            r#"(objects: [(type: "Tree", data: (texture: Asset("bush")))])"#,
        );
        assert!(matches!(error, SceneFileError::UnknownAssetName { name } if name == "bush"));

        let error = first_scene_error(
            "wrong_asset_kind",
            r#"(objects: [(type: "Tree", data: (texture: Asset("level_two")))])"#,
        );
        assert!(matches!(
            error,
            SceneFileError::WrongAssetKind {
                expected: "Texture",
                found: "SceneFile",
                ..
            }
        ));
    }

    #[test]
    fn data_that_does_not_match_its_type_names_the_object() {
        let text = r#"(objects: [(type: "Walker", id: "w", data: (sped: 3.0))])"#;
        match first_scene_error("bad_data", text) {
            SceneFileError::ObjectData { type_name, id, .. } => {
                assert_eq!(type_name, "Walker");
                assert_eq!(id.as_deref(), Some("w"));
            }
            error => panic!("{error:?}"),
        }
    }

    #[test]
    fn registering_a_name_twice_fails() {
        let mut scene_types = scene_types();
        assert_eq!(
            scene_types.register_object::<Hat>(),
            Err(RegisterSceneTypeError::NameTaken { name: "Hat" })
        );
    }

    #[test]
    fn references_can_only_be_read_while_a_scene_file_loads() {
        let Err(error) = ron::from_str::<Door>(r#"(walker: Object("walker"))"#) else {
            panic!("a reference was read outside a scene file load");
        };
        assert!(
            error
                .to_string()
                .contains("only be read while a scene file loads"),
            "{error}"
        );
    }

    /// Begins the next scene from a file on its first tick, and switches
    /// once everything it holds has finished loading.
    #[derive(Default)]
    struct ChangeToFile {
        scene_file: Option<SceneFileAsset>,
        counts_seen: Vec<ScopeLoadCounts>,
        switch_errors: Vec<QueueSceneChangeError>,
        file_errors: usize,
    }

    impl GameState for ChangeToFile {
        fn tick(&mut self, ctx: &mut GameStateTickContext) {
            if let Some(scene_file) = self.scene_file.take() {
                ctx.begin_next_scene_from_file(scene_file).unwrap();
            }
            if !ctx.has_next_scene() {
                return;
            }
            if ctx.next_scene_file_error().is_some() {
                self.file_errors += 1;
                ctx.discard_next_scene_deferred().unwrap();
                return;
            }
            let counts = ctx.next_scene_scope_load_counts().unwrap();
            self.counts_seen.push(counts);
            if let Err(error) = ctx.switch_to_next_scene_deferred() {
                self.switch_errors.push(error);
            }
        }
    }

    #[test]
    fn a_next_scene_from_a_file_is_built_when_read_and_switched_by_the_game() {
        let folder = folder_with(
            "next_scene",
            &[(
                "two.ron",
                r#"(scene_state: (type: "Level", data: (number: 2, player: Object("w"))),
                    objects: [(type: "Walker", id: "w", data: (speed: 1.0))])"#,
            )],
        );
        let game = ChangeToFile {
            scene_file: Some(LEVEL_TWO),
            ..ChangeToFile::default()
        };
        let mut core = Core::new(game, Scene::new(()), settings(folder));

        // The file is read in the background. Until it is built, the scene
        // can't be switched to.
        core.tick();
        assert!(core.next_scene().is_none());
        assert_eq!(
            core.game_state().switch_errors,
            vec![QueueSceneChangeError::NextSceneNotBuilt]
        );
        assert_eq!(core.game_state().counts_seen[0].loading, 1);

        // Once read, the next tick that ends builds it. The engine doesn't
        // switch: the scene is still the old one.
        core.block_until_pending_loads_finish();
        let ticks_until_built = (0..10)
            .position(|_| {
                core.tick();
                core.next_scene().is_some()
            })
            .expect("the read file is built when a tick ends");
        assert!(core.current_scene().scene_state::<Level>().is_none());
        assert!(ticks_until_built <= 1, "{ticks_until_built}");

        // The game's next tick sees it built, counts the file as ready,
        // and switches.
        core.tick();
        let last_counts = *core.game_state().counts_seen.last().unwrap();
        assert_eq!((last_counts.loading, last_counts.ready), (0, 1));
        assert_eq!(
            core.current_scene().scene_state::<Level>().unwrap().number,
            2
        );
        assert!(core.next_scene().is_none());
    }

    #[test]
    fn a_next_scene_file_that_fails_is_reported_and_discarded() {
        let game = ChangeToFile {
            scene_file: Some(MISSING),
            ..ChangeToFile::default()
        };
        let folder = folder_with("missing_next_scene", &[]);
        let mut core = Core::new(game, Scene::new(()), settings(folder));
        core.tick();
        core.block_until_pending_loads_finish();
        for _ in 0..3 {
            core.tick();
        }
        assert_eq!(core.game_state().file_errors, 1);
        assert!(core.next_scene().is_none());
        // The current scene kept running.
        assert_eq!(core.game_time().tick_count(), 4);
    }
}
