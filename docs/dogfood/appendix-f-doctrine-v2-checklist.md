# paceq recording doctrine

Single source of truth for what may be written into the paceq engineering
record. It governs Object Sections and Backlog points.

**How to use this document as a reviewer.** It is a checklist. Work items C1
through C12 in order, in writing, one at a time. For each item answer `PASS` or
`FAIL` and quote the exact words of the mutation that decide it. A review that
does not produce twelve answers is not a review of this doctrine. If any item is
`FAIL`, the whole review fails; say which item and why.

The requirements have not changed from the previous revision of this document.
Only the form has: what was prose is now numbered.

---

## Part A — what the mutation MUST contain

Check these first. Each one is a thing that has to be **present**. Wording that
is clean but missing one of these fails just as hard as wording that is dirty.

### C1. The quantity the decision is about is in the prose, in words

If the decision is about a number a person agreed to — a budget, a ceiling, a
bound, a count, a deadline — that number **must appear in the section's own
prose**, spelled as a word or a numeral.

- Test: cover every other field. Reading the prose alone, can you say what the
  number is?
- FAIL: "released after losing its turn a set number of times".
- PASS: "released after losing its turn three times".
- This item does not apply to a number that belongs to the implementation
  rather than to the decision (a buffer size, a timeout the code picked, an
  index). Those are C8 material. If you are unsure which kind it is, ask: did a
  person decide this number, or did the code? A number a person decided is C1.

### C2. A fact that lives in the source is carried by an implementation relation

If the assertion is about specific code, the mutation **must carry** an
implementation file association, and a symbol association where a symbol is what
implements it.

- Test: does the mutation have a `implemented_by` file relation? If the
  assertion names behaviour that one function provides, does it have the symbol
  relation too?
- FAIL: a section asserting how dispatch behaves, with an excerpt of the
  dispatch function and no relation.
- PASS: the same section with the file and symbol relations, and no excerpt
  needed at all.
- An excerpt is **not** a substitute for a relation. An excerpt is for showing a
  reader how something is spelled; a relation is for saying where it lives.

### C3. A fact that lives in another Section is carried by a reference

If the wording relies on what another Section asserts, the mutation **must**
depend on it through a selective reference naming the exact fields relied on.

- Test: does the prose restate, paraphrase or presuppose another Section? If
  yes, is there a `--ref` onto that Section and those fields?
- FAIL: a criterion that restates the decision it constrains, with no reference.
- PASS: the same criterion with a reference onto that decision's text and role.

### C4. The reason that makes the assertion decidable is present

- Test: can a reader who was not present act on this without asking anybody?
- FAIL: "the escape is exclusive." (Exclusive of what, and why?)
- PASS: a sentence that gives the reason the assertion follows.

---

## Part B — what the mutation MUST NOT contain

### C5. One assertion only

- Test: is there a second thing in this wording that a reader could separately
  agree or disagree with? If yes, it is two Sections.

### C6. No narration

No restating the problem, no history of how the team got here, no options not
taken, no description of what happens next.

### C7. No anticipation

No wording about what a later change might do. That is unresolved work and goes
to the backlog (C12).

### C8. No code-related material in the prose

The prose may not contain any of these. This list is exhaustive and each entry
is checkable by reading:

1. identifiers — module, class, function, method, variable, field or constant
   names, in any spelling or casing;
2. repository paths, file names, file extensions;
3. command lines, subcommands, flags, environment variable names;
4. literal values taken from source: string literals, regular expressions,
   status codes, and numbers that belong to the implementation (see C1 for the
   numbers that belong to the decision and MUST be in the prose);
5. code fences, inline code spans, any verbatim excerpt.

Where each kind belongs instead: an excerpt goes in the section's bounded
excerpt content; a file goes in the implementation file relation; a symbol goes
in the implementation symbol relation; the commit goes in the basis field.

### C9. No laundering

Material removed from the prose under C8 must have gone to the field that owns
it. Moving it into the header, the title, or an excerpt that does not fit it is
not a fix.

- Test: for every code-related thing the change is about, name which field now
  carries it. If the answer for any of them is "the header" or "the title", this
  item FAILS.

### C10. Not vague

Wording made so unspecific, in order to satisfy C8, that a reader can no longer
act on it, FAILS. Vagueness is not a safe answer to C8: C1 and C4 are the
directions it has to satisfy at the same time.

- Test: read the prose alone and state what a reader is now supposed to do or
  believe. If you cannot, this item FAILS, no matter how clean it is.

### C11. No duplication and no contradiction

- Nothing that this doctrine says is quoted back into the record.
- Nothing another live Section asserts is repeated here (use C3 instead).
- No two live Sections may disagree. If new wording makes an existing admitted
  Section untrue, that Section is revised or superseded in the same breath.

### C12. Settled, or it is not a Section at all

An open question, a suspicion, or anything whose answer nobody has settled is
not a Section. It is a backlog point.

---

## Part C — how these items apply to the backlog

Backlog points are checked against C1, C4, C5, C6, C8, C9 and C10 only.

C3 and C11's "no repeating another Section" do not apply: an unresolved point
may legitimately restate what a Section asserts in order to question it. C12
does not apply: being unsettled is the point. C2 applies as the subject
association rather than an implementation relation.

---

## Part D — the title

An Object title is navigation, not an assertion. It is checked against C8 and C9
only, and it must be short. Do not write a complete sentence with its reason
into a title; that belongs in a Section.
