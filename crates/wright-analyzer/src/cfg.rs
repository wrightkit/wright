//! Control-flow graphs over Workshop IR rules.
//!
//! [`Cfg`] flattens a rule's structured actions (If/While/ForGlobalVariable)
//! into basic blocks while preserving the structured semantics: branch edges,
//! loop back-edges, and loop exits are explicit [`EdgeKind`]s, so timing and
//! control-flow constructs are never flattened into misleading ordinary
//! sequential flow.
//!
//! Conservative approximations documented for v0.2:
//!
//! * Subroutine calls are kept as straight-line actions on a block; the
//!   callee's body is not inlined. Consumers can expand conservatively via
//!   [`Block::calls`] (the callee may contain branches, waits, or loops).
//! * Workshop `Skip`/goto-style flow is not representable in the v0.1 WIR and
//!   is therefore not modeled.
//! * Blocks containing a `wait` action are marked [`Block::waits`]; the
//!   analysis cannot decide how often a wait fires at runtime.

use std::collections::HashSet;

use wright_ir::arena::Arena;
use wright_ir::error::IrError;
use wright_ir::ids::{Id, IdLike};
use wright_ir::source::Span;
use wright_ir::wir::{self, Action, ActionId, GlobalVarId, RuleId, SubroutineId, ValueId};

/// A typed ID referencing a [`Block`].
pub type BlockId = Id<Block>;

/// A control-flow graph for one rule.
#[derive(Debug, Clone)]
pub struct Cfg {
    blocks: Arena<Block>,
    entry: BlockId,
    /// Synthesized exit block.
    exit: BlockId,
}

/// One basic block.
#[derive(Debug, Clone)]
pub struct Block {
    /// Straight-line actions contained in this block.
    pub actions: Vec<ActionId>,
    /// The control-flow action this block represents, for If/While/For
    /// blocks.
    pub source: Option<ActionId>,
    /// True when the block contains a `wait` action.
    pub waits: bool,
    /// Subroutines called from this block (conservative: not inlined).
    pub calls: Vec<SubroutineId>,
    pub kind: BlockKind,
    pub span: Option<Span>,
    pub successors: Vec<(BlockId, EdgeKind)>,
}

/// What a block represents in the structured program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// The rule's entry block.
    Entry,
    /// Ordinary straight-line code.
    StraightLine,
    /// An `If` condition evaluation; `BranchTrue`/`BranchFalse` successors.
    If { condition: ValueId },
    /// A `While` condition evaluation; `BranchTrue` (body) / `LoopExit`.
    While { condition: ValueId },
    /// A `ForGlobalVariable` header; body (Fallthrough) / `LoopExit`.
    ForHeader {
        variable: GlobalVarId,
        start: ValueId,
        stop: ValueId,
        step: ValueId,
    },
    /// The synthesized program exit.
    Exit,
}

/// The kind of a control-flow edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// Sequential flow.
    Fallthrough,
    /// The true branch of an If condition.
    BranchTrue,
    /// The false branch of an If condition.
    BranchFalse,
    /// A loop back-edge to its header.
    BackEdge,
    /// The edge out of a loop after it terminates.
    LoopExit,
}

impl Cfg {
    /// Build the CFG for one rule.
    pub fn build(program: &wir::Program, rule: RuleId) -> Result<Cfg, IrError> {
        let rule_data = program
            .rules
            .get(rule)
            .ok_or_else(|| dangling("rule", rule))?;
        Builder::new(program).build_rule(rule, &rule_data.actions)
    }

    /// The rule's entry block.
    pub fn entry(&self) -> BlockId {
        self.entry
    }

    /// The synthesized exit block.
    pub fn exit(&self) -> BlockId {
        self.exit
    }

    /// Iterate over all blocks in creation order.
    pub fn blocks(&self) -> impl Iterator<Item = BlockId> {
        (0..self.blocks.len()).map(BlockId::from_index)
    }

    /// The block with the given ID, if in range.
    pub fn block(&self, id: BlockId) -> Option<&Block> {
        self.blocks.get(id)
    }

    /// Successors of a block, in edge order.
    pub fn successors(&self, id: BlockId) -> &[(BlockId, EdgeKind)] {
        self.blocks
            .get(id)
            .map_or(&[], |block| block.successors.as_slice())
    }

    /// Predecessors of a block, derived from all edges.
    pub fn predecessors(&self, id: BlockId) -> Vec<(BlockId, EdgeKind)> {
        let mut result = Vec::new();
        for block in self.blocks() {
            for (successor, kind) in self.successors(block) {
                if *successor == id {
                    result.push((block, *kind));
                }
            }
        }
        result
    }

    /// Whether a block is reachable from the entry.
    pub fn reachable(&self, id: BlockId) -> bool {
        let mut seen = HashSet::new();
        let mut stack = vec![self.entry];
        while let Some(block) = stack.pop() {
            if !seen.insert(block) {
                continue;
            }
            if block == id {
                return true;
            }
            for (successor, _) in self.successors(block) {
                stack.push(*successor);
            }
        }
        false
    }

    /// Every loop header (While/For blocks), in creation order.
    pub fn loop_headers(&self) -> Vec<BlockId> {
        self.blocks()
            .filter(|block| {
                matches!(
                    self.block(*block).map(|b| &b.kind),
                    Some(BlockKind::While { .. } | BlockKind::ForHeader { .. })
                )
            })
            .collect()
    }

    /// Every block containing a `wait` action, in creation order.
    pub fn blocks_with_waits(&self) -> Vec<BlockId> {
        self.blocks()
            .filter(|block| self.block(*block).is_some_and(|b| b.waits))
            .collect()
    }

    /// The block containing the given action, if any. Control-flow actions
    /// map to their block via [`Block::source`]; straight-line actions via
    /// [`Block::actions`].
    pub fn action_block(&self, action: ActionId) -> Option<BlockId> {
        self.blocks().find(|block| {
            self.block(*block)
                .is_some_and(|b| b.source == Some(action) || b.actions.contains(&action))
        })
    }

    /// Render a deterministic text dump of the CFG.
    pub fn dump(&self, program: &wir::Program) -> String {
        let mut out = String::new();
        out.push_str("cfg\n");
        for block in self.blocks() {
            let data = self.block(block).expect("in range");
            out.push_str(&format!("block {block} {}\n", kind_name(&data.kind)));
            for action in &data.actions {
                let name = action_name(program, *action);
                out.push_str(&format!("  {name}\n"));
            }
            if data.waits {
                out.push_str("  [waits]\n");
            }
            for (successor, kind) in &data.successors {
                out.push_str(&format!("  -> {successor} ({})\n", edge_name(*kind)));
            }
        }
        out
    }
}

struct Builder<'a> {
    program: &'a wir::Program,
    cfg: Cfg,
}

impl<'a> Builder<'a> {
    fn new(program: &'a wir::Program) -> Self {
        Builder {
            program,
            cfg: Cfg {
                blocks: Arena::new(),
                entry: BlockId::from_index(0),
                exit: BlockId::from_index(0),
            },
        }
    }

    fn build_rule(&mut self, rule: RuleId, actions: &[ActionId]) -> Result<Cfg, IrError> {
        let span = self
            .program
            .rules
            .get(rule)
            .ok_or_else(|| dangling("rule", rule))?
            .span;
        let (entry, exit) = self.sequence(actions, BlockKind::Entry, span)?;
        let exit_block = self.new_block(BlockKind::Exit, span);
        self.edge(exit, exit_block, EdgeKind::Fallthrough)?;
        self.cfg.entry = entry;
        self.cfg.exit = exit_block;
        Ok(self.cfg.clone())
    }

    /// Process a sequence of actions, returning the entry block and the
    /// terminal block the caller connects onwards.
    fn sequence(
        &mut self,
        actions: &[ActionId],
        entry_kind: BlockKind,
        span: Option<Span>,
    ) -> Result<(BlockId, BlockId), IrError> {
        let entry = self.new_block(entry_kind, span);
        let mut current = entry;
        for action in actions {
            let data = self
                .program
                .actions
                .get(*action)
                .ok_or_else(|| dangling("action", *action))?
                .clone();
            match &data {
                Action::If {
                    branches,
                    else_body,
                    ..
                } => {
                    let merge = self.new_block(BlockKind::StraightLine, data.span());
                    let mut false_target: Option<BlockId> = None;
                    if let Some(else_body) = else_body {
                        let (else_entry, else_exit) =
                            self.sequence(else_body, BlockKind::StraightLine, data.span())?;
                        self.edge(else_exit, merge, EdgeKind::Fallthrough)?;
                        false_target = Some(else_entry);
                    }
                    for branch in branches.iter().rev() {
                        let condition = self
                            .program
                            .values
                            .get(branch.condition)
                            .ok_or_else(|| dangling("value", branch.condition))?;
                        let if_block = self.new_block(
                            BlockKind::If {
                                condition: branch.condition,
                            },
                            condition.span,
                        );
                        self.cfg
                            .blocks
                            .get_mut(if_block)
                            .expect("created above")
                            .source = Some(*action);
                        let (body_entry, body_exit) =
                            self.sequence(&branch.body, BlockKind::StraightLine, data.span())?;
                        self.edge(if_block, body_entry, EdgeKind::BranchTrue)?;
                        self.edge(body_exit, merge, EdgeKind::Fallthrough)?;
                        let target = false_target.unwrap_or(merge);
                        self.edge(if_block, target, EdgeKind::BranchFalse)?;
                        false_target = Some(if_block);
                    }
                    let first = false_target.expect("at least one branch");
                    self.edge(current, first, EdgeKind::Fallthrough)?;
                    current = merge;
                }
                Action::While {
                    condition, body, ..
                } => {
                    let condition_value = self
                        .program
                        .values
                        .get(*condition)
                        .ok_or_else(|| dangling("value", *condition))?;
                    let header = self.new_block(
                        BlockKind::While {
                            condition: *condition,
                        },
                        condition_value.span,
                    );
                    self.cfg
                        .blocks
                        .get_mut(header)
                        .expect("created above")
                        .source = Some(*action);
                    self.edge(current, header, EdgeKind::Fallthrough)?;
                    let (body_entry, body_exit) =
                        self.sequence(body, BlockKind::StraightLine, data.span())?;
                    self.edge(header, body_entry, EdgeKind::BranchTrue)?;
                    self.edge(body_exit, header, EdgeKind::BackEdge)?;
                    let after = self.new_block(BlockKind::StraightLine, data.span());
                    self.edge(header, after, EdgeKind::LoopExit)?;
                    current = after;
                }
                Action::ForGlobalVariable {
                    variable,
                    start,
                    stop,
                    step,
                    body,
                    ..
                } => {
                    let header = self.new_block(
                        BlockKind::ForHeader {
                            variable: *variable,
                            start: *start,
                            stop: *stop,
                            step: *step,
                        },
                        data.span(),
                    );
                    self.cfg
                        .blocks
                        .get_mut(header)
                        .expect("created above")
                        .source = Some(*action);
                    self.edge(current, header, EdgeKind::Fallthrough)?;
                    let (body_entry, body_exit) =
                        self.sequence(body, BlockKind::StraightLine, data.span())?;
                    self.edge(header, body_entry, EdgeKind::Fallthrough)?;
                    self.edge(body_exit, header, EdgeKind::BackEdge)?;
                    let after = self.new_block(BlockKind::StraightLine, data.span());
                    self.edge(header, after, EdgeKind::LoopExit)?;
                    current = after;
                }
                _ => {
                    // Straight-line action.
                    let block = self
                        .cfg
                        .blocks
                        .get_mut(current)
                        .expect("current block exists");
                    block.actions.push(*action);
                    if is_wait(&data) {
                        block.waits = true;
                    }
                    if let Action::CallSubroutine { subroutine, .. } = &data {
                        block.calls.push(*subroutine);
                    }
                }
            }
        }
        Ok((entry, current))
    }

    fn new_block(&mut self, kind: BlockKind, span: Option<Span>) -> BlockId {
        self.cfg.blocks.push(Block {
            actions: Vec::new(),
            source: None,
            waits: false,
            calls: Vec::new(),
            kind,
            span,
            successors: Vec::new(),
        })
    }

    fn edge(&mut self, from: BlockId, to: BlockId, kind: EdgeKind) -> Result<(), IrError> {
        let block = self
            .cfg
            .blocks
            .get_mut(from)
            .ok_or_else(|| dangling("block", from))?;
        block.successors.push((to, kind));
        Ok(())
    }
}

/// Whether an action is a `wait`.
fn is_wait(action: &Action) -> bool {
    matches!(action, Action::Call { name, .. } if name == "wait")
}

fn kind_name(kind: &BlockKind) -> &'static str {
    match kind {
        BlockKind::Entry => "entry",
        BlockKind::StraightLine => "block",
        BlockKind::If { .. } => "if",
        BlockKind::While { .. } => "while",
        BlockKind::ForHeader { .. } => "for",
        BlockKind::Exit => "exit",
    }
}

fn edge_name(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Fallthrough => "fallthrough",
        EdgeKind::BranchTrue => "true",
        EdgeKind::BranchFalse => "false",
        EdgeKind::BackEdge => "back",
        EdgeKind::LoopExit => "loop-exit",
    }
}

fn action_name(program: &wir::Program, action: ActionId) -> String {
    match program.actions.get(action) {
        Some(Action::Call { name, .. }) => format!("call {name}"),
        Some(Action::CallSubroutine { subroutine, .. }) => {
            let name = program
                .subroutines
                .get(*subroutine)
                .map_or_else(|| "<dangling>".to_string(), |s| s.name.clone());
            format!("callSubroutine {name}")
        }
        Some(Action::Debug { .. }) => "debug".to_string(),
        Some(Action::Print { .. }) => "print".to_string(),
        Some(Action::SetGlobalVariable { variable, .. }) => {
            let name = program
                .global_variables
                .get(*variable)
                .map_or_else(|| "<dangling>".to_string(), |v| v.name.clone());
            format!("setGlobalVariable {name}")
        }
        Some(Action::ModifyGlobalVariable { variable, op, .. }) => {
            let name = program
                .global_variables
                .get(*variable)
                .map_or_else(|| "<dangling>".to_string(), |v| v.name.clone());
            format!("modifyGlobalVariable {name} {}", op.as_str())
        }
        Some(Action::SetPlayerVariable { variable, .. }) => {
            let name = program
                .player_variables
                .get(*variable)
                .map_or_else(|| "<dangling>".to_string(), |v| v.name.clone());
            format!("setPlayerVariable {name}")
        }
        Some(Action::ModifyPlayerVariable { variable, op, .. }) => {
            let name = program
                .player_variables
                .get(*variable)
                .map_or_else(|| "<dangling>".to_string(), |v| v.name.clone());
            format!("modifyPlayerVariable {name} {}", op.as_str())
        }
        Some(Action::If { .. }) => "if".to_string(),
        Some(Action::While { .. }) => "while".to_string(),
        Some(Action::ForGlobalVariable { variable, .. }) => {
            let name = program
                .global_variables
                .get(*variable)
                .map_or_else(|| "<dangling>".to_string(), |v| v.name.clone());
            format!("forGlobalVariable {name}")
        }
        None => format!("<dangling action {action}>"),
    }
}

fn dangling(what: &'static str, id: impl IdLike) -> IrError {
    IrError::DanglingReference {
        what,
        id: id.index() as u32,
    }
}
