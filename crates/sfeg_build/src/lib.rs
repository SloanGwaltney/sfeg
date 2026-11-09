//! Build-script support for SFEG games (ADR 0015).
//!
//! A game lists its assets in a manifest that maps names it chooses to an
//! asset kind and a file, in RON:
//!
//! ```ron
//! {
//!     "player_idle": Texture("textures/player-idle.png"),
//! }
//! ```
//!
//! Its `build.rs` calls [`generate_asset_constants`], which checks the
//! manifest and writes a constant for each name, such as
//! `PLAYER_IDLE`. The game includes them in a module:
//!
//! ```ignore
//! mod assets {
//!     include!(concat!(env!("OUT_DIR"), "/sfeg_assets.rs"));
//! }
//! ```
//!
//! A misspelled name in code is then a compile error, and a manifest path
//! that doesn't exist fails the build.

use std::error::Error;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use serde::de::{MapAccess, Visitor};

/// The file the constants are written to, inside Cargo's `OUT_DIR`.
pub const GENERATED_FILE_NAME: &str = "sfeg_assets.rs";

/// Where the game's manifest and assets are, and how strictly to check
/// them, passed to [`generate_asset_constants`].
///
/// Relative paths are relative to the game's crate, where Cargo runs build
/// scripts.
///
/// Every field must be set. There is deliberately no `Default`, so each
/// choice is written in the game's `build.rs`, and a setting added in a
/// later SFEG version is a compile error until the game sets it.
///
/// ```
/// # use sfeg_build::{AssetManifestSettings, SharedFileCheck};
/// let settings = AssetManifestSettings {
///     manifest_path: "assets/assets.ron".into(),
///     asset_folder: "assets".into(),
///     files_shared_by_names: SharedFileCheck::Warn,
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetManifestSettings {
    /// The manifest file, such as `assets/assets.ron`.
    pub manifest_path: PathBuf,
    /// The folder the manifest's paths are relative to, such as `assets`.
    /// The game passes the same folder to `CoreSettings::asset_folder`.
    pub asset_folder: PathBuf,
    /// What to do when two or more names point at the same file. Each name
    /// is a separate asset, so the file is loaded once per name.
    /// [`SharedFileCheck::Warn`] suits most games.
    pub files_shared_by_names: SharedFileCheck,
}

/// What the build does when two or more manifest names point at the same
/// file.
///
/// Sharing never fails the build: each name is a valid asset. It is
/// usually a copy-paste mistake, though, and the file is loaded and kept
/// in memory once per name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedFileCheck {
    /// Print a Cargo warning for each shared file, naming every name that
    /// uses it. The build still succeeds.
    Warn,
    /// Allow it silently, for projects that give one file several names on
    /// purpose.
    Allow,
}

/// Checks the game's asset manifest and writes a constant for each asset
/// to `OUT_DIR`, in [`GENERATED_FILE_NAME`]. Call it from `build.rs`.
///
/// It also tells Cargo to rerun the build script when the manifest or
/// anything in the asset folder changes.
///
/// # Errors
///
/// - [`AssetBuildError::ReadManifest`] if the manifest can't be read.
/// - [`AssetBuildError::ParseManifest`] if it isn't valid manifest RON,
///   such as an unknown asset kind.
/// - [`AssetBuildError::InvalidManifest`] with every problem found, such
///   as a file that doesn't exist.
/// - [`AssetBuildError::OutDirNotSet`] if not run by Cargo as a build
///   script.
/// - [`AssetBuildError::WriteGenerated`] if the constants can't be
///   written.
pub fn generate_asset_constants(settings: &AssetManifestSettings) -> Result<(), AssetBuildError> {
    println!(
        "cargo::rerun-if-changed={}",
        settings.manifest_path.display()
    );
    // Cargo watches every file inside a folder given here.
    println!(
        "cargo::rerun-if-changed={}",
        settings.asset_folder.display()
    );

    let text = std::fs::read_to_string(&settings.manifest_path).map_err(|source| {
        AssetBuildError::ReadManifest {
            path: settings.manifest_path.clone(),
            source,
        }
    })?;
    let assets = check_manifest(&text, &settings.manifest_path, &settings.asset_folder)?;
    if settings.files_shared_by_names == SharedFileCheck::Warn {
        for shared in find_shared_files(&assets, &settings.asset_folder) {
            println!("cargo::warning={shared}");
        }
    }
    let code = generate_code(&settings.manifest_path, &assets);

    let out_dir = std::env::var_os("OUT_DIR").ok_or(AssetBuildError::OutDirNotSet)?;
    let path = Path::new(&out_dir).join(GENERATED_FILE_NAME);
    std::fs::write(&path, code).map_err(|source| AssetBuildError::WriteGenerated { path, source })
}

/// One asset the manifest lists, after checking.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ManifestAsset {
    name: String,
    kind: AssetKind,
    path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssetKind {
    Texture,
}

/// An entry's value in the manifest's RON.
#[derive(Deserialize)]
enum ManifestEntry {
    Texture(String),
}

/// The manifest's entries in file order, keeping duplicates so they can be
/// reported instead of one silently replacing the other.
struct ManifestEntries(Vec<(String, ManifestEntry)>);

impl<'de> Deserialize<'de> for ManifestEntries {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EntriesVisitor;

        impl<'de> Visitor<'de> for EntriesVisitor {
            type Value = ManifestEntries;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a map from asset names to assets")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ManifestEntries, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(ManifestEntries(entries))
            }
        }

        deserializer.deserialize_map(EntriesVisitor)
    }
}

/// Parses `text` and checks every entry, with paths relative to
/// `asset_folder`. Returns the assets sorted by name.
fn check_manifest(
    text: &str,
    manifest_path: &Path,
    asset_folder: &Path,
) -> Result<Vec<ManifestAsset>, AssetBuildError> {
    let ManifestEntries(entries) =
        ron::from_str(text).map_err(|source| AssetBuildError::ParseManifest {
            path: manifest_path.to_path_buf(),
            source: Box::new(source),
        })?;

    let mut problems = Vec::new();
    let mut assets: Vec<ManifestAsset> = Vec::new();
    for (name, entry) in entries {
        let (kind, path) = match entry {
            ManifestEntry::Texture(path) => (AssetKind::Texture, path),
        };
        if !is_valid_name(&name) {
            problems.push(ManifestProblem::InvalidName { name: name.clone() });
        } else if assets.iter().any(|asset| asset.name == name) {
            problems.push(ManifestProblem::DuplicateName { name: name.clone() });
        }
        if path.contains('\\') {
            problems.push(ManifestProblem::BackslashInPath {
                name: name.clone(),
                path: path.clone(),
            });
        } else if !stays_inside_folder(&path) {
            problems.push(ManifestProblem::PathOutsideAssetFolder {
                name: name.clone(),
                path: path.clone(),
            });
        } else if !asset_folder.join(&path).is_file() {
            problems.push(ManifestProblem::FileMissing {
                name: name.clone(),
                path: path.clone(),
            });
        }
        assets.push(ManifestAsset { name, kind, path });
    }
    if !problems.is_empty() {
        return Err(AssetBuildError::InvalidManifest {
            path: manifest_path.to_path_buf(),
            problems,
        });
    }
    assets.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(assets)
}

/// Lowercase ASCII letters, digits, and underscores, starting with a
/// letter, so every name becomes a distinct, valid constant name.
fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Whether `path` is relative and never climbs out with `..`.
fn stays_inside_folder(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

/// One file that two or more manifest names point at.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SharedFile {
    /// Every name pointing at it, in name order.
    names: Vec<String>,
    /// Each name's path as written, in the same order. They can differ,
    /// such as `a.png` and `./a.png`.
    paths: Vec<String>,
}

impl fmt::Display for SharedFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "one file is loaded under {} names:", self.names.len())?;
        for (name, path) in self.names.iter().zip(&self.paths) {
            write!(f, " {name:?} ({path:?})")?;
        }
        write!(
            f,
            ". Each name loads its own copy. If this is on purpose, set \
             files_shared_by_names to SharedFileCheck::Allow."
        )
    }
}

/// The files that more than one asset points at, comparing where the paths
/// lead on disk, so `a.png` and `./a.png` are the same file. Assets come
/// sorted by name, so the result is in a stable order.
fn find_shared_files(assets: &[ManifestAsset], asset_folder: &Path) -> Vec<SharedFile> {
    let mut by_file: Vec<(PathBuf, SharedFile)> = Vec::new();
    for asset in assets {
        // Every path was checked to exist, so this only fails if the file
        // vanished since. Such a file can't be shared with anything.
        let Ok(file) = asset_folder.join(&asset.path).canonicalize() else {
            continue;
        };
        match by_file.iter_mut().find(|(known, _)| *known == file) {
            Some((_, shared)) => {
                shared.names.push(asset.name.clone());
                shared.paths.push(asset.path.clone());
            }
            None => by_file.push((
                file,
                SharedFile {
                    names: vec![asset.name.clone()],
                    paths: vec![asset.path.clone()],
                },
            )),
        }
    }
    by_file
        .into_iter()
        .map(|(_, shared)| shared)
        .filter(|shared| shared.names.len() > 1)
        .collect()
}

fn generate_code(manifest_path: &Path, assets: &[ManifestAsset]) -> String {
    // Each constant allows being unused: an asset may be used only from
    // data files, such as scenes, never from code.
    let mut code = format!(
        "// @generated by sfeg_build from {}. Do not edit.\n",
        manifest_path.display()
    );
    for asset in assets {
        let constant = asset.name.to_ascii_uppercase();
        let (kind_doc, kind_type) = match asset.kind {
            AssetKind::Texture => ("texture", "TextureAsset"),
        };
        // `{:?}` writes a valid Rust string literal, escapes included.
        code.push_str(&format!(
            "\n/// The {kind_doc} `{name}`, from `{path}`.\n\
             #[allow(dead_code)]\n\
             pub const {constant}: ::sfeg::assets::{kind_type} =\n    \
             ::sfeg::assets::{kind_type}::from_manifest({name:?}, {path:?});\n",
            name = asset.name,
            path = asset.path,
        ));
    }
    code
}

/// One thing wrong with a manifest entry.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ManifestProblem {
    /// The name isn't lowercase letters, digits, and underscores starting
    /// with a letter.
    InvalidName {
        /// The name.
        name: String,
    },
    /// Another entry already has this name.
    DuplicateName {
        /// The name.
        name: String,
    },
    /// The path uses `\`. Paths use `/` on every platform, so the manifest
    /// works everywhere.
    BackslashInPath {
        /// The entry's name.
        name: String,
        /// Its path.
        path: String,
    },
    /// The path is empty, absolute, or climbs out with `..`, so it isn't
    /// inside the asset folder.
    PathOutsideAssetFolder {
        /// The entry's name.
        name: String,
        /// Its path.
        path: String,
    },
    /// No file exists at the path.
    FileMissing {
        /// The entry's name.
        name: String,
        /// Its path.
        path: String,
    },
}

impl fmt::Display for ManifestProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestProblem::InvalidName { name } => write!(
                f,
                "{name:?} isn't a valid name: use lowercase letters, digits, and underscores, \
                 starting with a letter"
            ),
            ManifestProblem::DuplicateName { name } => {
                write!(f, "{name:?} is listed more than once")
            }
            ManifestProblem::BackslashInPath { name, path } => {
                write!(f, "{name:?}: {path:?} uses \\; use / between folders")
            }
            ManifestProblem::PathOutsideAssetFolder { name, path } => write!(
                f,
                "{name:?}: {path:?} isn't a relative path inside the asset folder"
            ),
            ManifestProblem::FileMissing { name, path } => {
                write!(f, "{name:?}: no file at {path:?}")
            }
        }
    }
}

/// Why [`generate_asset_constants`] failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum AssetBuildError {
    /// The manifest couldn't be read, such as when it doesn't exist.
    ReadManifest {
        /// The manifest's path.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The manifest isn't a valid RON map from names to assets, such as
    /// when an entry uses an unknown asset kind.
    ParseManifest {
        /// The manifest's path.
        path: PathBuf,
        /// Where and why parsing failed.
        source: Box<ron::error::SpannedError>,
    },
    /// The manifest parsed, but has problems. Every problem is listed.
    InvalidManifest {
        /// The manifest's path.
        path: PathBuf,
        /// Each problem, in file order.
        problems: Vec<ManifestProblem>,
    },
    /// The `OUT_DIR` environment variable isn't set, which means this isn't
    /// running in a build script.
    OutDirNotSet,
    /// The generated constants couldn't be written.
    WriteGenerated {
        /// Where they were being written.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
}

impl fmt::Display for AssetBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssetBuildError::ReadManifest { path, source } => {
                write!(
                    f,
                    "couldn't read the asset manifest {}: {source}",
                    path.display()
                )
            }
            AssetBuildError::ParseManifest { path, source } => {
                write!(
                    f,
                    "the asset manifest {} isn't valid: {source}",
                    path.display()
                )
            }
            AssetBuildError::InvalidManifest { path, problems } => {
                write!(f, "the asset manifest {} has problems:", path.display())?;
                for problem in problems {
                    write!(f, "\n  - {problem}")?;
                }
                Ok(())
            }
            AssetBuildError::OutDirNotSet => {
                write!(f, "OUT_DIR isn't set; call this from a build script")
            }
            AssetBuildError::WriteGenerated { path, source } => {
                write!(f, "couldn't write {}: {source}", path.display())
            }
        }
    }
}

impl Error for AssetBuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            AssetBuildError::ReadManifest { source, .. }
            | AssetBuildError::WriteGenerated { source, .. } => Some(source),
            AssetBuildError::ParseManifest { source, .. } => Some(source.as_ref()),
            AssetBuildError::InvalidManifest { .. } | AssetBuildError::OutDirNotSet => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh asset folder holding `files`, each empty.
    fn asset_folder(test_name: &str, files: &[&str]) -> PathBuf {
        let folder = std::env::temp_dir()
            .join("sfeg_build_tests")
            .join(format!("{test_name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        for file in files {
            let path = folder.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"").unwrap();
        }
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn check(text: &str, folder: &Path) -> Result<Vec<ManifestAsset>, AssetBuildError> {
        check_manifest(text, Path::new("assets.ron"), folder)
    }

    fn problems(text: &str, folder: &Path) -> Vec<ManifestProblem> {
        match check(text, folder) {
            Err(AssetBuildError::InvalidManifest { problems, .. }) => problems,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_valid_manifest_gives_its_assets_sorted_by_name() {
        let folder = asset_folder("valid", &["textures/b.png", "a (2).png"]);
        let assets = check(
            r#"{ "tree": Texture("textures/b.png"), "player_idle": Texture("a (2).png") }"#,
            &folder,
        )
        .unwrap();
        let names: Vec<&str> = assets.iter().map(|asset| asset.name.as_str()).collect();
        assert_eq!(names, ["player_idle", "tree"]);
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let folder = asset_folder("problems", &["here.png"]);
        let found = problems(
            r#"{
                "Bad Name": Texture("here.png"),
                "twice": Texture("here.png"),
                "twice": Texture("here.png"),
                "gone": Texture("missing.png"),
                "climbs": Texture("../here.png"),
                "absolute": Texture("/here.png"),
                "windows": Texture("sub\\here.png"),
            }"#,
            &folder,
        );
        assert_eq!(
            found,
            [
                ManifestProblem::InvalidName {
                    name: "Bad Name".into()
                },
                ManifestProblem::DuplicateName {
                    name: "twice".into()
                },
                ManifestProblem::FileMissing {
                    name: "gone".into(),
                    path: "missing.png".into()
                },
                ManifestProblem::PathOutsideAssetFolder {
                    name: "climbs".into(),
                    path: "../here.png".into()
                },
                ManifestProblem::PathOutsideAssetFolder {
                    name: "absolute".into(),
                    path: "/here.png".into()
                },
                ManifestProblem::BackslashInPath {
                    name: "windows".into(),
                    path: "sub\\here.png".into()
                },
            ]
        );
    }

    #[test]
    fn a_folder_is_not_a_file() {
        let folder = asset_folder("folder", &["textures/a.png"]);
        assert_eq!(
            problems(r#"{ "textures": Texture("textures") }"#, &folder),
            [ManifestProblem::FileMissing {
                name: "textures".into(),
                path: "textures".into()
            }]
        );
    }

    #[test]
    fn unknown_kinds_fail_to_parse() {
        let folder = asset_folder("unknown_kind", &["a.ogg"]);
        assert!(matches!(
            check(r#"{ "music": Sound("a.ogg") }"#, &folder),
            Err(AssetBuildError::ParseManifest { .. })
        ));
    }

    #[test]
    fn names_sharing_a_file_are_found_however_the_path_is_written() {
        let folder = asset_folder("shared", &["textures/tree.png", "rock.png"]);
        let assets = check(
            r#"{
                "tree": Texture("textures/tree.png"),
                "rock": Texture("rock.png"),
                "tree_again": Texture("./textures/tree.png"),
            }"#,
            &folder,
        )
        .unwrap();
        assert_eq!(
            find_shared_files(&assets, &folder),
            [SharedFile {
                names: vec!["tree".into(), "tree_again".into()],
                paths: vec!["textures/tree.png".into(), "./textures/tree.png".into()],
            }]
        );
    }

    #[test]
    fn files_with_one_name_each_are_not_shared() {
        let folder = asset_folder("not_shared", &["a.png", "b.png"]);
        let assets = check(
            r#"{ "a": Texture("a.png"), "b": Texture("b.png") }"#,
            &folder,
        )
        .unwrap();
        assert!(find_shared_files(&assets, &folder).is_empty());
    }

    #[test]
    fn the_shared_file_warning_says_how_to_allow_it() {
        let shared = SharedFile {
            names: vec!["tree".into(), "tree_again".into()],
            paths: vec!["tree.png".into(), "./tree.png".into()],
        };
        assert_eq!(
            shared.to_string(),
            "one file is loaded under 2 names: \"tree\" (\"tree.png\") \
             \"tree_again\" (\"./tree.png\"). Each name loads its own copy. If this is \
             on purpose, set files_shared_by_names to SharedFileCheck::Allow."
        );
    }

    #[test]
    fn names_are_lowercase_snake_case() {
        assert!(is_valid_name("player_idle_2"));
        for name in ["", "2d", "_x", "Player", "a-b", "a b", "é"] {
            assert!(!is_valid_name(name), "{name:?}");
        }
    }

    #[test]
    fn generated_constants_quote_their_strings() {
        let code = generate_code(
            Path::new("assets/assets.ron"),
            &[ManifestAsset {
                name: "player_idle".into(),
                kind: AssetKind::Texture,
                path: "textures/player \"idle\".png".into(),
            }],
        );
        assert!(code.starts_with("// @generated by sfeg_build from assets/assets.ron."));
        // Assets may be used only from data files, so an unused constant
        // isn't a mistake.
        assert!(code.contains(
            "#[allow(dead_code)]\npub const PLAYER_IDLE: ::sfeg::assets::TextureAsset ="
        ));
        assert!(code.contains(
            r#"::sfeg::assets::TextureAsset::from_manifest("player_idle", "textures/player \"idle\".png");"#
        ));
    }
}
