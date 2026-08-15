# P6 findings — named/default argument binding against catalog signatures

Reference: OSTW v3.4.0 (identity in `probe.json`). Evidence:
`result.entry-only.json`, `workshop.entry-only.txt`.

## Observed facts

- `WorkshopSettingToggle(SortOrder: 5, Default: true, Category: "6. Interface",
  Name: "Highlight hovered hero (4 texts)")` (deliberately REORDERED) →
  `Workshop Setting Toggle(Custom String("6. Interface"), Custom String("Highlight hovered hero (4 texts)"), True, 5)`
  — canonical order (Category, Name, Default, SortOrder).
- `CreateInWorldText(VisibleTo:, Header:, Position:, Clipping:, Scale:,
  Reevaluation:, Spectators:)` with `Scale:` AFTER `Clipping:` in source and
  `Color` omitted → `Create In-World Text(All Players(All Teams), Custom String, Subtract(Left, Left), 3, Do Not Clip, Visible To And String, Color(White), Visible Always)`
  — Scale=3 lands before Clipping; Color defaults to `Color(White)`.
- `BigMessage(Header: …)` (Player omitted) → `Big Message(All Players(All Teams), …)`.
- `StartCamera(Player:, EyePosition: Vector(0,0,0), LookAtPosition: Vector(1,0,0))`
  → `Start Camera(Event Player, Subtract(Left, Left), Left, 0)` — a 4th parameter
  exists and takes its default 0.
- `userCall(C: 9, A: 1)` (user function, `in Number A=1, B=2, C=3`) →
  `Modify Global Variable(counter, Add, 12)` — C=9 + A=1 + B default 2, folded.
- `IsButtonHeld(Button: Button.Ability2)` → `Is Button Held(Event Player, Button(Ability 2))`;
  `SetInvisible(InvisibleTo: InvisibleTo.All)` → `Set Invisible(Event Player, All)`
  — omitted player params default to `Event Player`.
- `CreateHudText(VisibleTo: toggle ? AllPlayers() : null, Header:, Location:,
  SortOrder:, HeaderColor:, Reevaluation:, Spectators:)` →
  `Create HUD Text(If-Then-Else(toggle, All Players(All Teams), Null), Custom String("banned"), Null, Null, Left, 1, Color(Red), Color(White), Color(White), Visible To, Visible Always)`
  — Subheader/Text default to Null, colors default White.
- Positional controls: `Wait(0.05, WaitBehavior.AbortWhenFalse)` →
  `Wait(0.05, Abort When False)`; `SmallMessage(EventPlayer(), "p6 done")` →
  `Small Message(Event Player, Custom String("p6 done"))`;
  `BigMessage(AllPlayers(), …)` → `Big Message(All Players(All Teams), …)`.
- Vector formatting: `Vector(0,0,0)` → `Subtract(Left, Left)`; `Vector(1,0,0)` →
  `Left` (scratch observation; `Vector(-1,0,0)` → `Right`).
- accept, elementCount 68, 0 diagnostics, workshopCode SHA-256 `0e94c4a3…3e28`.

## Decision support (interpretation)

- Named arguments bind BY NAME regardless of source order, against canonical
  signature parameter order; omitted parameters take their defaults. The
  canonical emitted argument order IS the signature order — this is the
  ordering a Wright-owned catalog must assign the reachable graph's calls
  (WorkshopSettingToggle, CreateInWorldText, CreateHudText, BigMessage,
  StartCamera, IsButtonHeld, SetInvisible, Wait, SmallMessage).
- Receiver defaults observed: `All Players(All Teams)` for BigMessage,
  `Event Player` for IsButtonHeld/SetInvisible. User-defined functions bind
  named args and defaults identically (userCall C:9,A:1 → 1+2+9).
- Vector spellings (`Subtract(Left, Left)`, `Left`) are reference output-syntax
  / constant-folding artifacts (N-level), NOT semantics: do not copy them into
  Wright emission.

## Caveats

- The exact signature parameter NAMES/orders for the catalog entries are
  observed from the canonical output, not from upstream data; each catalog entry
  added for #118 needs its own pinned probe of this shape.
- `userCall` folding (12) is an optimizer artifact; the named-binding outcome is
  the evidence.
