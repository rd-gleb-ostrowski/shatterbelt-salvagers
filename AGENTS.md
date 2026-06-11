## General hints

- `cat` is symlinked to `bat`, if you want to read `cat` output,
  use `cat -pp --color=never` to disable interactive features.
- `.gitignore` is deny per default, if you create new files & folders,
  which should be versioned, they need to be added as exclusions to `.gitignore`.
  Try to use wildcards if it makes sense
  (e.g., most likely, all files under a `src` folder should be included).
