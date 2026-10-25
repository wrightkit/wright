# ADR-0036: `wright ui` dependency policy

- Status: Superseded by [ADR-0037](0037-web-editor-experiment.md) (decision 5 carries the dependency policy to the `editor` repository; the paths this record links are retired)
- Date: 2026-10-22
- Related: [ADR-0032](0032-structured-editor-form-factor.md), [UI design](../ui/README.md)

## Context

ADR-0032 decision 7 allows one runtime dependency, `svelte`, and rules out a component library, a CSS framework, a router, and a state library. The first layers kept to that rule. The result is hand-written behavior that mature libraries already provide:

- `Picker.svelte` positions its popover by hand and implements its own listbox keyboard handling.
- `dialog.ts` wraps the native `<dialog>`. Nested layers, such as a picker over the editor dialog, need their own Esc and focus rules.
- Each icon is an SVG path drawn by hand inside `SqButton`.

The next layers need more of the same kind of behavior:

- a context menu (#917);
- a hover card (#924);
- a full combobox picker (#919);
- drag and drop of rules, conditions, and actions, which is not designed yet.

Each of these has accessibility and positioning rules (focus, keyboard, screen readers, viewport collision) that are easy to get wrong and are already solved upstream. Writing them by hand costs more code than the dependency would, and every hand-written copy can differ from the others.

At the same time, the UI must not collect overlapping libraries. Two libraries for one concern give two behaviors and two upgrade paths for one problem.

## Decision

This decision replaces the "Dependencies" bullet of ADR-0032 decision 7. The rest of decision 7 (Svelte 5 runes, TypeScript strict, Vite, snapshots at the boundary) stands.

1. **Prefer a mature library over hand-written behavior.** A library qualifies when it:
   - supports Svelte 5 runes natively, not through a compatibility layer;
   - has had a release in the last six months;
   - has an MIT, ISC, Apache-2.0, or BSD license;
   - implements the WAI-ARIA pattern for its component, where one exists;
   - keeps the bundle inside the ADR-0032 decision 10 budget.

2. **One library per concern.** `docs/ui/dependencies.md` lists each concern and the one library that owns it. A second library for a listed concern is not added. Replacing a listed library is a new decision that removes the old one in the same change.

3. **No hand-written component or behavior that an existing dependency expresses correctly.** "Existing dependency" includes Svelte itself. Hand-written behavior is allowed only where the owning library cannot express it, and the PR says why.

4. **Styled source is ours.** Third-party styles and themes do not enter `ui/`.
   - Components in `ui/src/lib/components/` wrap the library's headless parts. They style them with the tokens in `docs/ui/visual-system.md`, in component-scoped `<style>` blocks.
   - Feature components use these wrappers. They do not import the library directly.
   - So the design language is defined in one layer, and replacing a library changes only that layer.

5. **The initial registry:**

   | Concern | Owner |
   | --- | --- |
   | Reactivity and state | Svelte runes (`$state`, `$derived`, classes in `.svelte.ts`) |
   | Transitions and motion | Svelte (`svelte/transition`, `svelte/animate`, `svelte/motion`) |
   | Layout | CSS grid and flex |
   | Interactive primitives: dialog, popover, combobox, menu, context menu, tooltip, hover card, checkbox | Bits UI |
   | Icons | Lucide (`@lucide/svelte`) |
   | Drag and drop | Not chosen. Chosen when its issue is designed, by comparing candidates on a real large list. |

   A state library, an animation library, and a layout library are not added: rows 1–3 already cover those concerns.

6. **New runtime dependencies.** The PR that adds one states the need, checks the registry, and updates `docs/ui/dependencies.md` in the same change. CI fails when `ui/package.json` has a runtime dependency that the registry does not list.

## Alternatives considered

- **Keep ADR-0032's single-dependency rule.** Rejected. It makes each new interactive component a hand-written accessibility and positioning implementation, which is more code and more risk than a dependency.
- **shadcn-svelte as delivered.** Rejected as delivered; its model is kept. It copies component source into the project, which matches decision 4. But its components are styled with Tailwind and its own default look, and this UI follows the in-game editor through its own tokens. Wrapping Bits UI directly gives the same source ownership without a second styling system.
- **Tailwind CSS.** Rejected for now. Utility classes could enforce token use. But together with component-scoped styles they would be two styling paths for one concern, which decision 2 rules out.
- **Melt UI, Ark UI, or Zag.js** for primitives. Rejected. Bits UI covers every primitive the spec needs today, with a component API that fits the wrapper layer.
- **An icon set through unplugin-icons or Iconify.** Rejected. It gives access to many sets, and one set is the goal. Lucide's 24 px stroke icons match the existing `SqButton` shell, so no new build step is needed.

## Consequences

- Picker positioning, dialog layering, and listbox keyboard handling move from hand-written code to Bits UI.
- `docs/ui/dependencies.md` becomes the current record of which library owns which concern.
- The bundle grows. Bits UI depends on Floating UI, `runed`, `tabbable`, and `svelte-toolbelt`. The 100 KB budget is unchanged; if the wrappers do not fit, that is reported, not hidden by raising the budget.
- The `docs/ui/README.md` statement "WrightKit draws its own icons" changes to "WrightKit uses Lucide icons". The rule that no game or community asset enters `ui/` is unchanged.
- Follow-up work: adopting Bits UI behind the wrapper layer; moving styles into components; adopting Lucide.

## Compatibility impact

None for users or agents. `wright-agent/v1`, the CLI, and the binary distribution are unchanged. The UI's look and behavior stay as specified in `docs/ui/`.

## Scope boundaries

This ADR does not decide:

- the drag-and-drop library or interaction;
- a virtual list for very large columns;
- changes to the size budget.
