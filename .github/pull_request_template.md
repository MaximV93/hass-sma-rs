<!--
Thanks for sending a PR! Skim the checklist below before you submit.
-->

## Summary

<!-- One sentence on what this changes. Don't explain WHAT the diff
     does (reviewer reads the diff) — explain WHY. -->

## Rationale

<!-- What problem did this solve? Link to the issue if there is one. -->

## Testing

<!-- How did you verify this works? -->

- [ ] `cargo test --workspace` green
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] `cargo fmt --all -- --check` clean
- [ ] Added/updated tests for this change
- [ ] Manual testing against real inverter (tell us which model + firmware)

## Risk + rollback

<!-- If this breaks production, how do you un-break? Mention the
     addon version that last worked if this is a user-facing change. -->

## Checklist

- [ ] CHANGELOG.md updated
- [ ] Docs updated (README / GETTING_STARTED / ARCHITECTURE / ADR as appropriate)
- [ ] If schema changed: addon `config.yaml` schema section updated in the companion hassio-addons repo
- [ ] No AI-attribution lines in commit messages (`Co-Authored-By: ...AI...`)

## Related ADRs / issues

<!-- Link ADRs you're extending or issues you're closing. "Fixes #42" /
     "See ADR 0005". -->
