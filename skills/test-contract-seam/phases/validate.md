# Validate Test Evidence

Run the focused suite and required repository gates against the stable diff.
Confirm every accepted claim has real-seam evidence, all material dimensions
are proven or justified as inapplicable, and each named sensitivity break is
detected. Verify required author/review receipts only where the admitted policy
or repository adapter requires them.

Record commands, results, source revision, limitations, and blockers in the same
evidence map. A skipped required lane, unavailable boundary, failed mutation,
uncovered claim, or stale evidence blocks completion. Do not start another
author/review cycle for unchanged evidence.

If validation exposes a production defect, preserve the test and finding and
return the defect to its owner. After repair, refresh only invalidated evidence
and repeat the affected gates.
