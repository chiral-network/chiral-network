# Reputation System (Current)

This document reflects the current Elo-based reputation model in the app.

## Scale

- Range: `0` to `100`
- Base score for new users: `50`

## Inputs

The score is driven by recent activity (last 180 days):

1. File transfer outcomes (completed or failed)
2. Amount of CHI earned from completed transfers (logarithmic weighting)

Older events have lower weight than recent events.

## Event Effects

- Successful file transfer: positive Elo adjustment
- Failed file transfer: negative Elo adjustment
- Higher earned value (recent): additional positive contribution via amount weighting

## Elo Formula

For each event within the lookback window:

1. **Time weight** (`w_time`): linear decay from 1.0 (today) to 0.0 (180 days ago)
2. **Amount weight** (`w_amount`): `1.0 + clamp(ln(1 + chi) / ln(51), 0, 1)` — ranges from 1.0 (free) to 2.0 (50+ CHI)
3. **Outcome**: 1.0 for completed, 0.0 for failed
4. **Expected score**: `1 / (1 + 10^((50 - elo) / 12))`
5. **K factor**: `4 * w_time * w_amount`
6. **Update**: `elo = clamp(elo + K * (outcome - expected), 0, 100)`

## Time Decay

Only events in the most recent 180 days are considered. Within that window, event weight decays linearly as event age increases, so recent behavior dominates.

## Fresh Start Policy

Historical legacy rating data was reset to start fresh. Current scores are computed from new events recorded under this system. The previous user rating system (1-5 stars) has been removed.

## Integrity Direction

The implementation tracks events through backend APIs and avoids trusting frontend-only edits for score calculation. Paid transfers are verified on-chain before being recorded. Future hardening can include stronger cryptographic attestation for each reputation event.
