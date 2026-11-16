## What and why

<!-- What changes for the user, and why. Link the issue: "Closes #12". -->

## How it was checked

<!-- Tests added, platforms tried, what could not be verified. No real tracks, places or health data. -->

## Checklist

- [ ] `cargo test --locked` passes
- [ ] A new feature has its path in `src/app/tests.rs`; pure functions are tested in their module
- [ ] Interface texts use `tr("English", "Français")`
- [ ] For a visible change: the page of the guide (`book/src/`), the changelog, and the README or `docs/index.html` if worth a line there
- [ ] No new dependency, or it was agreed in the issue
- [ ] Layers still depend downwards (`layers_only_depend_downwards`)
