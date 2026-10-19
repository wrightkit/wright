/**
 * The page's view model: `rules?rule&content` returns the owner's
 * `workshop-rs/rule-content-v1` document with Wright's statement spans
 * added — the generated `Condition`/`Action` types do not declare `span`,
 * so the spanned forms live here.
 */
import type {
  Action,
  Condition,
  WorkshopRsRuleContentV1,
} from './types/rule-content-v1'
import type {
  RuleSummary,
  Span,
  ToolErrorInfo,
  ToolResponse,
} from './types/wright-agent-v1'

export type SpannedCondition = Condition & { span?: Span }
export type SpannedAction = Action & { span?: Span }

export interface RuleContent extends Omit<WorkshopRsRuleContentV1, 'conditions' | 'actions'> {
  conditions: SpannedCondition[]
  actions: SpannedAction[]
}

/**
 * One rule's load state: `refused` carries the service refusal verbatim —
 * a rule whose content `rules` cannot express renders that refusal in
 * place while other rules still render (#804).
 */
export type RuleState =
  | { phase: 'loading' }
  | { phase: 'ready'; content: RuleContent }
  | { phase: 'refused'; refusal: ToolErrorInfo }

/** `?debug` on the session URL exposes node diagnostics (ADR-0032). */
export function debugEnabled(search: string): boolean {
  return new URLSearchParams(search).has('debug')
}

/** Map a `rules?…&content` summary to its view state. */
export function ruleStateFromSummary(summary: RuleSummary): RuleState {
  const content = summary.content as RuleContent | undefined
  return content
    ? { phase: 'ready', content }
    : {
        phase: 'refused',
        refusal: {
          code: 'rule-content-unavailable',
          message: 'the response carried no rule content',
        },
      }
}

/**
 * Map one `rules?rule&content` response to its view state: a refused call
 * keeps the refusal verbatim, a missing rule or missing content degrades
 * to the same refusal shape so the pane always renders something.
 */
export function ruleStateFromResponse(response: ToolResponse): RuleState {
  if ('error' in response) {
    return { phase: 'refused', refusal: response.error as ToolErrorInfo }
  }
  const result = response.result
  const summary =
    result && typeof result === 'object' && 'rules' in result
      ? (result.rules as RuleSummary[])[0]
      : undefined
  return summary
    ? ruleStateFromSummary(summary)
    : {
        phase: 'refused',
        refusal: {
          code: 'rule-content-unavailable',
          message: 'the response carried no rule',
        },
      }
}

/** The unreachable-endpoint state `rules` reads degrade to. */
export function ruleStateFromError(error: unknown): RuleState {
  return {
    phase: 'refused',
    refusal: {
      code: 'endpoint-unreachable',
      message: error instanceof Error ? error.message : String(error),
    },
  }
}

export { type RuleSummary, type Span, type ToolErrorInfo }
