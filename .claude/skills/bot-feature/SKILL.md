---
name: bot-feature
description: Do a piece of work on an agent/* branch and open a PR as sfeg-bot.
---

# Description

Does a piece of work on its own `agent/<name>` branch, then pushes it and opens a PR as the sfeg-bot GitHub App. Commits are authored by the user. The user reviews and merges.

# Steps

1. Check that `~/.config/sfeg-bot/token.mjs` exists. If it doesn't, stop and tell the user the bot isn't set up on this machine.
2. Update `main` (`git switch main && git pull`), then create `agent/<short-kebab-name>` from it.
3. Do the work. Commit in small, focused commits with the user's git identity. Never change the git author.
4. Push with `git push -u origin agent/<name>`. The git credential helper sends it as sfeg-bot.
5. Open the PR as sfeg-bot: `GH_TOKEN=$(~/.config/sfeg-bot/token.mjs) gh pr create --base main`, with a short summary and the attribution footer.
6. Send all later `gh` actions (comments, review replies) with `GH_TOKEN` the same way.
7. Never merge, close, or push to `main`. Give the user the PR link.
