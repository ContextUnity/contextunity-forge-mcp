# Author Contract-Seam Tests

Follow the admitted policy for timing and required author/reviewer roles. Cover
the scenarios and dimensions in the single evidence map; the complete test set
must prove every accepted claim, not merely one mocked function.

Enter through a supported production caller, traverse real collaborators, and
assert the observed consumer result and claimed state/effect at the production
boundary. Use fakes only behind unavailable external dependencies outside that
boundary. Do not call private helpers or replace in-boundary behavior with mocks
for contract-seam proof; classify useful local tests as supplemental.

Keep the test and fixture diff within repository-declared paths. If proving the
seam needs production, contract, adapter, or migration changes, preserve the
finding and return it to the owning delivery role. Do not invent historical red
for a test written after implementation.
