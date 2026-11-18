# Dynamic segment splitting (work stealing)

- **Status:** accepted

The `Segment` scheduler dynamically splits the largest remaining active `Segment` whenever a `Worker` finishes early and the remaining un-downloaded range exceeds the minimum `Segment` size floor, rather than assigning fixed static ranges for the lifetime of a `Download`.

## Considered Options

- **Static equal-sized `Segment`s:** Simple to reason about, but a single throttled or stalled connection leaves a long tail where all other `Worker`s sit idle.
- **Dynamic segment splitting:** Keeps all `Worker`s saturated until completion, at the cost of requiring coordinated `Segment` end-offset updates between the scheduler and active `Worker`s.
