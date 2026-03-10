# Reputation System (Current)

This document reflects the current Elo-based reputation model in the app.

## Scale

- Range: `0` to `100`
- Base score for new users: `50`

## Inputs

The score is driven by recent activity (last 180 days):

1. File-provider transaction outcomes
2. Money earned from completed seeding transactions
3. User ratings (`1-5`)

Older events have lower weight than recent events.

## Event Effects

- Successful file-provider transaction: positive Elo adjustment
- Failed/negative file-provider outcome: negative Elo adjustment
- Higher earned value (recent): additional positive contribution
- Low user ratings: negative contribution
- High user ratings: positive contribution

## Time Decay

Only events in the most recent 180 days are considered.

Within that window, event weight decays as event age increases, so recent behavior dominates.

## Fresh Start Policy

Historical legacy rating data was reset to start fresh. Current scores are computed from new events recorded under this system.

## Integrity Direction

The implementation tracks events through backend APIs and avoids trusting frontend-only edits for score calculation. Future hardening can include stronger cryptographic attestation for each reputation event.
