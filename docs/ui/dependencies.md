# `wright ui` dependencies

Context: part of the `wright ui` design ([README](README.md)). ADR-0036 sets the rules. This page is the current registry: one owner per concern.

| Concern | Owner | Wrapper |
| --- | --- | --- |
| Reactivity and state | Svelte runes | — |
| Transitions and motion | `svelte/transition`, `svelte/animate`, `svelte/motion` | — |
| Layout | CSS grid and flex | — |
| Interactive primitives | `bits-ui` | `ui/src/lib/components/` |
| Icons | `@lucide/svelte` | `SqButton` |
| Drag and drop | Not chosen | — |

Before adding a runtime dependency, check this table. If its concern is listed, use the listed owner. If not, add a row in the same PR and state the need.
