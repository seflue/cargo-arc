# Comment Standards

Applies to every language.

Every comment passes three gates, in this order: **is it needed, is it true,
does it land.** The order is the rule. A comment that fails the first gate is
deleted, not reworded, and a comment nobody verified is not improved by better
phrasing.

## Not covered

These are not prose. They stay, even where they restate the code:

- annotations a tool consumes: `@param`, `@return`, type hints, anything a doc
  generator publishes
- suppressions and pragmas: lint disables, `noqa`, `expect-error`, `#pragma`
- licence headers, copyright, generated-file banners
- `TODO` / `FIXME` carrying a ticket reference

A prose description attached to one of these passes the gates like any other
comment. If one of them is factually wrong, that is a defect to report, not a
comment to rewrite into prose.

## Gate 1: is it needed

The comment must have a consequence to report. If you cannot name what breaks,
or what the code would otherwise do, it is decoration.

Delete when:

- the name already answers it
- it restates the line below it, including a doc-comment lead that paraphrases
  its own body (which also rots silently, because nothing fails when the body
  changes underneath it)
- it is the story of the debugging session, which belongs in the commit message

**The caller test.** When a comment explains what some consumer does with this
code rather than the code itself, ask: *if that caller disappeared, would this
code still be here?* If yes, the caller is decoration and the comment should
describe the thing. If no, the caller is the justification and naming it is the
only way a reader can judge whether the line is still needed. Swallowed errors,
defensive guards and hardcoded invariants usually land on the second side.

The gate runs per comment, and on comments you touched as well as ones you
wrote. A comment left standing beside rewritten code is a comment you are now
asserting is still true.

## Gate 2: is it true

Every claim must be checkable by the reader: a named symbol, a documentation
reference, a file and function in a dependency.

- **Separate observed from inferred.** A failure mode read out of a library's
  source is a *would*, not a *does*. Writing it in the indicative invents a bug.
- **One check per claim.** Read the doc, grep the symbol, run one small probe.
  If that does not settle it, say the claim is unverified and write only what
  is known. Do not build a harness to rescue a comment.
- **Never soften a fact you failed to check.** When the claim is knowledge
  rather than reasoning (a business rule, a workaround for a third-party bug, a
  reason rooted in history), ask the author. Watering it down is how the fact
  gets lost.
- Claims about another branch or a future change do not belong here.

## Gate 3: does it land

Read the comment with the rest of the file covered. Anything that dangles is a
defect.

- **Self-contained.** No pronoun or back-reference whose antecedent is missing
  from the comment and from the line it sits on.
- **Roles match the code.** Use a role noun only where the surrounding code
  binds that name to the same thing.
- **No term that costs a round trip.** If a reader would have to ask what a
  word means here, write the concrete thing instead.
- **One causal step per sentence.** A colon joining two inferences is where the
  second one hides.
- **Name the rejected alternative** when the comment justifies a choice.
  Otherwise "instead" and "rather than" have no referent.
- **Doc comments: imperative verb first, what then why.** `Delete the cached
  entries, to keep each refresh from leaking one`, not `Every refresh builds a
  fresh entry, so without this ...`. State the contract (what goes in, what
  comes out, what is guaranteed), not the algorithm.
- **Inline comments** sit above the construct they explain and restate its
  condition in plain words.
- One or two lines.

## Stop rule

A comment that needs a third rewrite is not a wording problem. Either it has no
consequence to report, and gate 1 should have caught it, or the code is
unclear and the code is what should change.

## TODO without a ticket

Three ways out, and leaving it is not one:

1. Fix it now, if it is in the scope of the current change.
2. File the item and write the ID in: `TODO(cef-0155): batch these queries`.
3. Drop the comment and carry the gap into the handover, so a human decides
   whether it becomes a ticket.

An unreferenced TODO is a private note in shared code. No owner, no expiry, no
way to be found again.

## Examples

```
Bad:  // Increment counter
      counter += 1
Good: no comment.

Bad:  // Set timeout to 30 seconds
      timeout = 30
Good: // 30s matches the upstream gateway idle-close; longer values cause
      // spurious 504s.
      timeout = 30

Bad:  /// This function parses the config file and returns the parsed config.
Good: /// Returns the merged config. Fails if the file exists but is malformed;
      /// a missing file yields defaults.

Bad:  // TODO: handle the error case properly
Good: // TODO(cef-0155): retry on 429 instead of failing the batch
```
