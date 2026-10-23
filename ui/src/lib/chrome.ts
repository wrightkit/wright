/**
 * UI chrome strings (docs/ui/design.md § Language): the WrightKit-owned
 * text — title, column headings, dialog titles, button tooltips, and
 * empty states — keyed by session locale, never taken from the game.
 * A locale without a table falls back to en-US.
 */

/** The chrome strings one locale provides; every key is required. */
export interface ChromeStrings {
  title: string
  rulesToolbar: string
  rulesRegion: string
  rulesCount: (count: number) => string
  diagnostics: string
  onlyRulesWithFindings: string
  severityName: (severity: 'error' | 'warning' | 'info') => string
  variables: string
  pendingChanges: string
  filterPlaceholder: string
  language: string
  addRuleLabel: string
  addRuleTitle: string
  loadingRules: string
  noRulesMatch: string
  loadingRule: string
  contentRefused: string
  event: string
  team: string
  player: string
  subroutine: string
  conditions: string
  actions: string
  none: string
  conditionNoun: string
  actionNoun: string
  selectAll: (noun: string) => string
  selectRow: (noun: string, index: number) => string
  addStatement: (noun: string) => string
  addStatementTitle: (noun: string) => string
  selectedCount: (count: number) => string
  selectedHiddenCount: (count: number, hidden: number) => string
  filterColumn: (noun: string) => string
  disable: string
  enable: string
  delete: string
  clear: string
  statementMenu: (noun: string, index: number) => string
  selectSameAction: string
  selectBlock: string
  expandRule: string
  collapseRule: string
  expandRuleLabel: (name: string) => string
  collapseRuleLabel: (name: string) => string
  findingMarkLabel: (severity: string, count: number) => string
  loading: string
  findingsUnavailable: string
  noVariables: string
  noDiagnostics: string
  noPending: string
  close: string
  confirm: string
  discardAll: string
  cancel: string
  submit: string
  applying: string
  conditionTitle: string
  actionLabel: string
  disabled: string
  ok: string
  unset: string
  trueLiteral: string
  falseLiteral: string
  simulation: string
  simTeam1: string
  simTeam2: string
  simYou: string
  simSlot: string
  simNoHero: string
  simLobbySettings: string
  simSeconds: string
  simStart: string
  simRunning: string
  simRefused: string
  simNotVerified: string
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
  addStatementTitle: (noun) => `Add ${noun}`,
  selectedCount: (count) => `${count} selected`,
  selectedHiddenCount: (count, hidden) => `${count} selected (${hidden} hidden)`,
  filterColumn: (noun) => `Filter ${noun}s…`,
  disable: 'Disable',
  enable: 'Enable',
  delete: 'Delete',
  clear: 'Clear',
  statementMenu: (noun, index) => `${noun} ${index} menu`,
  selectSameAction: 'Select same action',
  selectBlock: 'Select block',
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
  actionLabel: 'Action',
  disabled: 'Disabled',
  ok: 'OK',
  unset: 'unset',
  trueLiteral: 'True',
  falseLiteral: 'False',
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
  addStatementTitle: (noun) => `添加${noun}`,
  selectedCount: (count) => `已选择 ${count} 项`,
  selectedHiddenCount: (count, hidden) => `已选择 ${count} 项（${hidden} 项已隐藏）`,
  filterColumn: (noun) => `筛选${noun}…`,
  disable: '禁用',
  enable: '启用',
  delete: '删除',
  clear: '清除',
  statementMenu: (noun, index) => `${noun} ${index} 菜单`,
  selectSameAction: '选择相同动作',
  selectBlock: '选择块',
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
  actionLabel: '动作',
  disabled: '禁用',
  ok: '确定',
  unset: '未设置',
  trueLiteral: '真',
  falseLiteral: '假',
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
