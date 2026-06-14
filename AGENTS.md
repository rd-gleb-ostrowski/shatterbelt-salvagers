## General hints

- `.gitignore` is deny per default, if you create new files & folders,
  which should be versioned, they need to be added as exclusions to `.gitignore`.
  Try to use wildcards if it makes sense
  (e.g., most likely, all files under a `src` folder should be included).
- Never commit files you did not touch or create.

## Agent skills

### Issue tracker

Issues and PRDs live as markdown files under `.scratch/<feature>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Five canonical roles, default strings, recorded as a `Status:` line in each issue file. See `docs/agents/triage-labels.md`.

### Domain docs

Multi-context: `CONTEXT-MAP.md` at the root points to per-context `CONTEXT.md` files. See `docs/agents/domain.md`.
