## Summary

Summarize the changes in plain language and include the motivation/context.

## Linked Issue(s)

Link the related issue(s). Example: `Closes #123`.

## aivcs linkage

Run `aivcs pr-note` and paste the output below for PRs that capture agent cognitive state.

For **code/docs-only** changes with no cognitive snapshot (typical for bot/Jules rustdoc or CLI-only fixes), use an exempt sentinel instead of a real CommitId:

```
aivcs-commit: code-only-docs-change-no-cognitive-snapshot
```

Other examples: `code-only-cli-change-no-cognitive-snapshot`. Pattern: `code-only-<scope>-no-cognitive-snapshot`.

## Checklist

- [ ] My code follows the style guidelines of this project
- [ ] New and existing unit tests pass locally with my changes
