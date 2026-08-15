# P3c findings — player-variable receiver access

Reference: OSTW v3.4.0 (identity in `probe.json`). Evidence:
`result.entry-only.json`, `workshop.entry-only.txt`.

## Observed facts

- `playervar Number p;` (no explicit ID) → table `player: 0: p`.
- In an `Event.OngoingPlayer` rule:
  - `EventPlayer().p = 5;` → `Set Player Variable(Event Player, p, 5);`
  - `EventPlayer().p += 2;` → `Modify Player Variable(Event Player, p, Add, 2);`
  - `sink = EventPlayer().p;` → `Set Global Variable(sink, Player Variable(Event Player, p));`
- accept, elementCount 15, 0 diagnostics, workshopCode SHA-256 `fb82fe65…145`.

## Decision support (interpretation)

- The player-variable receiver `EventPlayer().p` lowers to the Workshop player
  variable pair (receiver = `Event Player`, variable = `p`) for read, plain
  write, and compound write (`+=` → Modify Add). This is exactly the HIR/WIR
  `PlayerVar { player, variable }` shape the inventory proposes.
- Auto player-variable allocation uses index 0 for the first playervar.

## Caveats

- Structural evidence; no runtime check that `Event Player` is the intended
  player (the receiver expression in the reference is `EventPlayer()`, which the
  reference maps 1:1 to `Event Player`).
