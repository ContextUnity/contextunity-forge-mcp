# Discover Test Evidence

Build the single evidence map before authoring. Follow the admitted policy's
timing; `seam-test-first` does not require an implementation revision or
implementation review.

For every accepted claim, map its scenario to the supported production entry,
real collaborators, claimed boundary, observable outcome, and relevant existing
tests. Identify all material success, rejection, malformed-input, limit,
security, failure, persistence, concurrency, alternate-caller, and external-
effect dimensions; record inapplicable dimensions with a reason.

Name one plausible contract-breaking mutation per material contract scenario and the
production evidence level needed to detect it. Do not infer a real seam from a
mocked unit test. Keep supplemental tests clearly classified and list a
replacement only when an old test is actually subsumed.
