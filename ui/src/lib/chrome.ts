/**
 * UI chrome strings (docs/ui/design.md § Language): the WrightKit-owned
 * text — title, column headings, dialog titles, button tooltips, and
 * empty states — keyed by session locale, never taken from the game.
 * A locale without a table falls back to en-US.
 */

/** The chrome strings one locale provides; every key is required. */
export interface ChromeStrings {
  /** The title row's large italic name. */
  title: string
  /** The toolbar's aria label. */
  rulesToolbar: string
  /** The rule-list section's aria label. */
  rulesRegion: string
  /** The toolbar's rule count. */
  rulesCount: (count: number) => string
  /** The Diagnostics button tooltip and dialog title. */
  diagnostics: string
  /** The "Only rules with findings" toolbar toggle's tooltip. */
  onlyRulesWithFindings: string
  /** A severity group's heading name in the Diagnostics dialog. */
  severityName: (severity: 'error' | 'warning' | 'info') => string
  /** The Variables button tooltip and dialog title. */
  variables: string
  /** The pending-changes button tooltip and dialog title. */
  pendingChanges: string
  /** The rule-name filter's placeholder. */
  filterPlaceholder: string
  /** The locale selector's aria label. */
  language: string
  /** The Add rule button's aria label. */
  addRuleLabel: string
  /** The Add rule button's tooltip, naming why it is disabled. */
  addRuleTitle: string
  /** The rule list's loading empty state. */
  loadingRules: string
  /** The rule list's empty-filter state. */
  noRulesMatch: string
  /** An open rule's loading state. */
  loadingRule: string
  /** The refused-rule body's heading (the refusal text is the service's). */
  contentRefused: string
  /** Event-row labels, in the in-game order. */
  event: string
  team: string
  player: string
  subroutine: string
  /** The two statement column headings. */
  conditions: string
  actions: string
  /** An empty statement column's row. */
  none: string
  /** The singular nouns aria labels use for a conditions/actions row. */
  conditionNoun: string
  actionNoun: string
  /** The column select-all checkbox aria label. */
  selectAll: (noun: string) => string
  /** A statement row's checkbox aria label. */
  selectRow: (noun: string, index: number) => string
  addStatement: (noun: string) => string
  addStatementTitle: (noun: string) => string
  selectedCount: (count: number) => string
  selectedHiddenCount: (count: number, hidden: number) => string
  filterColumn: (noun: string) => string
  /** The heading's batch actions. */
  disable: string
  enable: string
  delete: string
  clear: string
  /** The rule-bar chevron tooltips. */
  expandRule: string
  collapseRule: string
  /** The rule-bar chevron aria labels. */
  expandRuleLabel: (name: string) => string
  collapseRuleLabel: (name: string) => string
  /** A severity mark's aria label: the severity and the finding count. */
  findingMarkLabel: (severity: string, count: number) => string
  /** The dialogs' loading state. */
  loading: string
  /** The findings surfaces' unavailable state (a refusal or transport error). */
  findingsUnavailable: string
  /** The Variables dialog's empty state. */
  noVariables: string
  /** The Diagnostics dialog's empty state. */
  noDiagnostics: string
  /** The pending-changes dialog's empty state. */
  noPending: string
  /** The view dialogs' close button. */
  close: string
  /** The view dialogs' footer confirm button. */
  confirm: string
  /** The pending-changes footer's buttons. */
  discardAll: string
  cancel: string
  submit: string
  applying: string
  /** The statement editor's title for a condition (actions title by name). */
  conditionTitle: string
  /** The statement editor's disable toggle. */
  disabled: string
  /** The statement editor's confirm button. */
  ok: string
  /** The argument hover card's slot line for a call parameter (§ Hover card). */
  argParameterSlot: (index: number, label: string | null, call: string) => string
  /** The argument hover card's slot lines for an infix operator's operands. */
  argOperandLeft: string
  argOperandRight: string
  /** The scope word the hover card puts before a variable's kind. */
  argScope: (scope: 'global' | 'player') => string
  /** The simulation button's tooltip and the panel's title (#836). */
  simulation: string
  /** The two room column headings. */
  simTeam1: string
  simTeam2: string
  /** The user's slot label; other slots use `simSlot` with a number. */
  simYou: string
  simSlot: string
  /** An empty hero dropdown's placeholder. */
  simNoHero: string
  /** The lobby settings section's heading. */
  simLobbySettings: string
  /** The run-length field's label. */
  simSeconds: string
  /** The Start button's labels, running and idle. */
  simStart: string
  simRunning: string
  /** A refusal's fallback message when the service sends none. */
  simRefused: string
  /** The wording `not_modeled` and `missing_input` outcomes carry. */
  simNotVerified: string
  /** The result section's headings. */
  simVariables: string
  simLogs: string
}

const enUS: ChromeStrings = {
  title: 'Workshop',
  rulesToolbar: 'Rules toolbar',
  rulesRegion: 'Rules',
  rulesCount: (count) => `Rules (${count})`,
  diagnostics: 'Diagnostics',
  onlyRulesWithFindings: 'Only rules with findings',
  severityName: (severity) =>
    severity === 'error' ? 'Errors' : severity === 'warning' ? 'Warnings' : 'Info',
  variables: 'Variables',
  pendingChanges: 'Pending changes',
  filterPlaceholder: 'Filter rules…',
  language: 'Language',
  addRuleLabel: 'Add rule',
  addRuleTitle: 'Add rule: rule-level edits are not available',
  loadingRules: 'Loading rules…',
  noRulesMatch: 'No rules match.',
  loadingRule: 'Loading rule…',
  contentRefused: "The service could not express this rule's content.",
  event: 'Event',
  team: 'Team',
  player: 'Player',
  subroutine: 'Subroutine',
  conditions: 'Conditions',
  actions: 'Actions',
  none: 'none',
  conditionNoun: 'condition',
  actionNoun: 'action',
  selectAll: (noun) => `select all ${noun}s`,
  selectRow: (noun, index) => `select ${noun} ${index}`,
  addStatement: (noun) => `Add ${noun}`,
  addStatementTitle: (noun) => `Add ${noun}: insertion is not available`,
  selectedCount: (count) => `${count} selected`,
  selectedHiddenCount: (count, hidden) => `${count} selected (${hidden} hidden)`,
  filterColumn: (noun) => `Filter ${noun}s…`,
  disable: 'Disable',
  enable: 'Enable',
  delete: 'Delete',
  clear: 'Clear',
  expandRule: 'Expand rule',
  collapseRule: 'Collapse rule',
  expandRuleLabel: (name) => `Expand ${name}`,
  collapseRuleLabel: (name) => `Collapse ${name}`,
  findingMarkLabel: (severity, count) =>
    `${severity}: ${count} ${count === 1 ? 'finding' : 'findings'}`,
  loading: 'Loading…',
  findingsUnavailable: 'Diagnostics are unavailable:',
  noVariables: 'No variables or subroutines declared.',
  noDiagnostics: 'No diagnostics.',
  noPending: 'No pending changes.',
  close: 'Close',
  confirm: 'Confirm',
  discardAll: 'Discard all',
  cancel: 'Cancel',
  submit: 'Submit',
  applying: 'Applying…',
  conditionTitle: 'Condition',
  disabled: 'Disabled',
  ok: 'OK',
  argParameterSlot: (index, label, call) =>
    `parameter ${index + 1}${label ? ` (${label})` : ''} of ${call}`,
  argOperandLeft: 'left operand',
  argOperandRight: 'right operand',
  argScope: (scope) => (scope === 'player' ? 'player' : 'global'),
  simulation: 'Simulation',
  simTeam1: 'Team 1',
  simTeam2: 'Team 2',
  simYou: 'You',
  simSlot: 'Slot',
  simNoHero: 'No hero',
  simLobbySettings: 'Lobby settings',
  simSeconds: 'Simulated seconds',
  simStart: 'Start',
  simRunning: 'Running…',
  simRefused: 'The service could not run the simulation.',
  simNotVerified: 'not verified',
  simVariables: 'Variables',
  simLogs: 'Log',
}

const zhCN: ChromeStrings = {
  title: '地图工坊',
  rulesToolbar: '规则工具栏',
  rulesRegion: '规则',
  rulesCount: (count) => `规则（${count}）`,
  diagnostics: '诊断',
  onlyRulesWithFindings: '仅有发现的规则',
  severityName: (severity) =>
    severity === 'error' ? '错误' : severity === 'warning' ? '警告' : '信息',
  variables: '变量',
  pendingChanges: '待定更改',
  filterPlaceholder: '筛选规则…',
  language: '语言',
  addRuleLabel: '添加规则',
  addRuleTitle: '添加规则：规则级编辑不可用',
  loadingRules: '正在加载规则…',
  noRulesMatch: '没有匹配的规则。',
  loadingRule: '正在加载规则…',
  contentRefused: '服务无法表达此规则的内容。',
  event: '事件',
  team: '队伍',
  player: '玩家',
  subroutine: '子程序',
  conditions: '条件',
  actions: '动作',
  none: '无',
  conditionNoun: '条件',
  actionNoun: '动作',
  selectAll: (noun) => `选择所有${noun}`,
  selectRow: (noun, index) => `选择${noun} ${index}`,
  addStatement: (noun) => `添加${noun}`,
  addStatementTitle: (noun) => `添加${noun}：插入功能不可用`,
  selectedCount: (count) => `已选择 ${count} 项`,
  selectedHiddenCount: (count, hidden) => `已选择 ${count} 项（${hidden} 项已隐藏）`,
  filterColumn: (noun) => `筛选${noun}…`,
  disable: '禁用',
  enable: '启用',
  delete: '删除',
  clear: '清除',
  expandRule: '展开规则',
  collapseRule: '折叠规则',
  expandRuleLabel: (name) => `展开 ${name}`,
  collapseRuleLabel: (name) => `折叠 ${name}`,
  findingMarkLabel: (severity, count) => `${severity}：${count} 项发现`,
  loading: '正在加载…',
  findingsUnavailable: '诊断不可用：',
  noVariables: '没有声明变量或子程序。',
  noDiagnostics: '没有诊断。',
  noPending: '没有待定更改。',
  close: '关闭',
  confirm: '确定',
  discardAll: '全部放弃',
  cancel: '取消',
  submit: '提交',
  applying: '正在应用…',
  conditionTitle: '条件',
  disabled: '禁用',
  ok: '确定',
  argParameterSlot: (index, label, call) =>
    `参数 ${index + 1}${label ? `（${label}）` : ''}，属于 ${call}`,
  argOperandLeft: '左操作数',
  argOperandRight: '右操作数',
  argScope: (scope) => (scope === 'player' ? '玩家' : '全局'),
  simulation: '模拟',
  simTeam1: '队伍 1',
  simTeam2: '队伍 2',
  simYou: '你',
  simSlot: '位置',
  simNoHero: '无英雄',
  simLobbySettings: '大厅设置',
  simSeconds: '模拟秒数',
  simStart: '开始',
  simRunning: '正在运行…',
  simRefused: '服务无法运行模拟。',
  simNotVerified: '未验证',
  simVariables: '变量',
  simLogs: '日志',
}

// Keys are lowercase: the service spells locales `zh-cn`/`en-us`.
const TABLES: Record<string, ChromeStrings> = {
  'en-us': enUS,
  'zh-cn': zhCN,
}

/** The chrome table for a session locale, en-US when none is translated. */
export function chromeStrings(locale: string): ChromeStrings {
  return TABLES[locale.toLowerCase()] ?? enUS
}

/**
 * Each catalog locale's own-language name, shown in the selector whatever
 * the session locale is; a code the table does not know renders as itself.
 * Keys are lowercase, matching the service's spellings.
 */
export const LOCALE_NAMES: Record<string, string> = {
  'en-us': 'English',
  'de-de': 'Deutsch',
  'es-es': 'Español (España)',
  'es-mx': 'Español (México)',
  'fr-fr': 'Français',
  'it-it': 'Italiano',
  'ja-jp': '日本語',
  'ko-kr': '한국어',
  'pl-pl': 'Polski',
  'pt-br': 'Português (Brasil)',
  'ru-ru': 'Русский',
  'th-th': 'ไทย',
  'tr-tr': 'Türkçe',
  'zh-cn': '简体中文',
  'zh-tw': '繁體中文',
}

/** A catalog locale's own-language name for the selector. */
export function localeName(code: string): string {
  return LOCALE_NAMES[code.toLowerCase()] ?? code
}
