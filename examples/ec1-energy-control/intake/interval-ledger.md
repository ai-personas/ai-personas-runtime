# Independent ledger: three synthetic EC1 returns

Nominal R1 = R2 = 1000 ohms, C1 = 0.001 F. Times in seconds; energies in mJ. A segment contains only adjacent samples on the supplied 200 Hz grid (including two-sided, zero-duration switch edges). A missing grid span splits segments. No missing charge or energy has been estimated. Phase labels below describe voltage patterns in these ideal exercises; they are not observations of switch position.

At each sample, use i=(IN−STORE)/R1. Positive port power is into R1. Integrate IN·i, its positive and negative magnitudes separately, R1·i² and LOAD²/R2 by endpoint trapezoids within each segment. U=½C·STORE². Closure = signed port − R1 heat − R2 load − (U_end−U_start), not an output. Last column = R2 load − inward port, not net generation.

## log-14.csv — HOLD — precharged window, no initial empty or charge

1802 rows; SHA-256 `5cc62432e03e787b0fe65f29b9bb95df8f1d1f361274158928157faaf3999c22`. Two-sided edge times: 1 s.

Observed: isolated 0–1 s (STORE 4.9876062→4.9876062 V); discharge 1–9 s (STORE 4.9876062→0.0016731555 V).

| Segment (s) | signed port | inward | outward | R1 heat | R2 load | U start | U end | closure | load − inward |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0–9 | 0 | 0 | 0 | 0 | 12.4382102 | 12.438108 | 1.39972466e-06 | -0.000103651287 | 12.4382102 |

## log-27.csv — USABLE — complete synthetic exercise

3204 rows; SHA-256 `73feecfcfe5a5ade00fda15c3d802c6817e6e2bfb0eef4e7505cd01bb2743af2`. Two-sided edge times: 0, 6, 8 s.

Observed: empty 0–0 s (STORE 0→0 V); charge 0–6 s (STORE 0→4.9876062 V); isolated 6–8 s (STORE 4.9876062→4.9876062 V); discharge 8–16 s (STORE 4.9876062→0.0016731555 V).

| Segment (s) | signed port | inward | outward | R1 heat | R2 load | U start | U end | closure | load − inward |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0–16 | 24.9380831 | 24.9380831 | 0 | 12.5000274 | 12.4382102 | 0 | 1.39972466e-06 | -0.000155862281 | -12.4998729 |

## log-83.csv — HOLD — samples missing

2705 rows; SHA-256 `6068205cb926227b96d4faf100281c9213ad2806d28921a7722b20c5793b53f4`. Two-sided edge times: 0, 6, 8 s.
Uncovered interval 0.25 → 2.75 s (2.5 s; 499 interior samples absent on this grid). Do not integrate across it or call segment subtotals full-cycle energy.

Observed: empty 0–0 s (STORE 0→0 V); charge 0–0.25 s (STORE 0→1.1059961 V).

Observed: charge 2.75–6 s (STORE 4.6803607→4.9876062 V); isolated 6–8 s (STORE 4.9876062→4.9876062 V); discharge 8–16 s (STORE 4.9876062→0.0016731555 V).

| Segment (s) | signed port | inward | outward | R1 heat | R2 load | U start | U end | closure | load − inward |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0–0.25 | 5.52999194 | 5.52999194 | 0 | 4.91840774 | 0 | 0 | 0.61161367 | -2.94659244e-05 | -5.52999194 |
| 2.75–16 | 1.53623093 | 1.53623093 | 0 | 0.0510082654 | 12.4382102 | 10.9528881 | 1.39972466e-06 | -0.000100875033 | 10.9019793 |

Positive load energy in a post-charge window draws down initial field energy. For a gapped record the endpoint store energies remain meaningful as state estimates, but their difference does not reveal the missing port input, load delivery or losses. These are arithmetic checks on ideal synthetic voltages, not calibrated measurements; there is no force, motion or all-input energy channel.
