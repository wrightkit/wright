use serde_json::{Value as JsonValue, json};
use workshop_rs::source::Span;
use workshop_rs::{Action, Program};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Fallthrough,
    BranchTrue,
    BranchFalse,
    BackEdge,
    LoopExit,
}

impl EdgeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fallthrough => "fallthrough",
            Self::BranchTrue => "true",
            Self::BranchFalse => "false",
            Self::BackEdge => "back",
            Self::LoopExit => "loop-exit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    Entry,
    StraightLine,
    If,
    While,
    ForHeader,
    Exit,
}

impl BlockKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Entry => "entry",
            Self::StraightLine => "block",
            Self::If => "if",
            Self::While => "while",
            Self::ForHeader => "for",
            Self::Exit => "exit",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Block {
    pub id: usize,
    pub kind: BlockKind,
    pub actions: Vec<usize>,
    pub waits: bool,
    pub calls: Vec<usize>,
    pub span: Option<Span>,
    pub successors: Vec<(usize, EdgeKind)>,
}

#[derive(Debug, Clone)]
pub struct Cfg {
    entry: usize,
    exit: usize,
    blocks: Vec<Block>,
}

impl Cfg {
    pub fn entry(&self) -> usize {
        self.entry
    }

    pub fn exit(&self) -> usize {
        self.exit
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    pub fn to_json(&self) -> JsonValue {
        let blocks = self
            .blocks
            .iter()
            .map(|b| {
                json!({
                    "id": b.id,
                    "kind": b.kind.as_str(),
                    "waits": b.waits,
                    "calls": b.calls,
                    "actions": b.actions,
                    "successors": b.successors.iter().map(|(to, kind)| json!({
                        "to": to,
                        "kind": kind.as_str(),
                    })).collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        json!({
            "entry": self.entry,
            "exit": self.exit,
            "blocks": blocks,
        })
    }

    pub fn build(program: &Program, rule_idx: usize) -> Result<Self, String> {
        let Some(rule) = program.rules.get(rule_idx) else {
            return Err(format!("unknown rule {rule_idx}"));
        };
        let mut builder = CfgBuilder {
            program,
            rule_idx,
            actions: &rule.actions,
            blocks: Vec::new(),
        };
        Ok(builder.build())
    }
}

struct CfgBuilder<'a> {
    program: &'a Program,
    rule_idx: usize,
    actions: &'a [Action],
    blocks: Vec<Block>,
}

impl<'a> CfgBuilder<'a> {
    fn new_block(&mut self, kind: BlockKind) -> usize {
        let id = self.blocks.len();
        self.blocks.push(Block {
            id,
            kind,
            actions: Vec::new(),
            waits: false,
            calls: Vec::new(),
            span: None,
            successors: Vec::new(),
        });
        id
    }

    fn add_action_to_block(&mut self, block_id: usize, action_idx: usize) {
        let block = &mut self.blocks[block_id];
        block.actions.push(action_idx);
        let action = &self.actions[action_idx];
        if is_action_wait(action) {
            block.waits = true;
        }
        if let Some(sub) = action_subroutine(action) {
            if let Some(idx) = self.program.subroutines.iter().position(|s| s.name == sub) {
                if !block.calls.contains(&idx) {
                    block.calls.push(idx);
                }
            }
        }
    }

    fn build(&mut self) -> Cfg {
        let entry = self.new_block(BlockKind::Entry);
        let exit = self.new_block(BlockKind::Exit);
        let first = self.build_range(0, self.actions.len(), exit);
        self.blocks[entry].successors.push((first, EdgeKind::Fallthrough));
        Cfg {
            entry,
            exit,
            blocks: std::mem::take(&mut self.blocks),
        }
    }

    fn build_range(&mut self, start: usize, end: usize, after: usize) -> usize {
        if start >= end {
            return after;
        }
        let mut current_start = start;
        let mut first_block = None;
        let mut prev_block: Option<(usize, EdgeKind)> = None;

        while current_start < end {
            match &self.actions[current_start] {
                Action::While { .. } => {
                    let matching_end = self.find_matching_end(current_start, end);
                    let header = self.new_block(BlockKind::While);
                    self.add_action_to_block(header, current_start);
                    self.blocks[header].span = self.program.action_span(self.rule_idx, current_start);

                    if let Some((prev, kind)) = prev_block.take() {
                        self.blocks[prev].successors.push((header, kind));
                    }
                    if first_block.is_none() {
                        first_block = Some(header);
                    }

                    let next_after = self.build_range(matching_end + 1, end, after);
                    let body_entry = self.build_range(current_start + 1, matching_end, header);
                    self.blocks[header].successors.push((body_entry, EdgeKind::BranchTrue));
                    self.blocks[header].successors.push((next_after, EdgeKind::LoopExit));
                    return first_block.unwrap_or(header);
                }
                Action::ForGlobalVariable { .. } | Action::ForPlayerVariable { .. } => {
                    let matching_end = self.find_matching_end(current_start, end);
                    let header = self.new_block(BlockKind::ForHeader);
                    self.add_action_to_block(header, current_start);
                    self.blocks[header].span = self.program.action_span(self.rule_idx, current_start);

                    if let Some((prev, kind)) = prev_block.take() {
                        self.blocks[prev].successors.push((header, kind));
                    }
                    if first_block.is_none() {
                        first_block = Some(header);
                    }

                    let next_after = self.build_range(matching_end + 1, end, after);
                    let body_entry = self.build_range(current_start + 1, matching_end, header);
                    self.blocks[header].successors.push((body_entry, EdgeKind::Fallthrough));
                    self.blocks[header].successors.push((next_after, EdgeKind::LoopExit));
                    return first_block.unwrap_or(header);
                }
                Action::If { .. } => {
                    let (else_idx, matching_end) = self.find_if_branches(current_start, end);
                    let if_block = self.new_block(BlockKind::If);
                    self.add_action_to_block(if_block, current_start);
                    self.blocks[if_block].span = self.program.action_span(self.rule_idx, current_start);

                    if let Some((prev, kind)) = prev_block.take() {
                        self.blocks[prev].successors.push((if_block, kind));
                    }
                    if first_block.is_none() {
                        first_block = Some(if_block);
                    }

                    let next_after = self.build_range(matching_end + 1, end, after);
                    if let Some(else_idx) = else_idx {
                        let true_entry = self.build_range(current_start + 1, else_idx, next_after);
                        let false_entry = self.build_range(else_idx + 1, matching_end, next_after);
                        self.blocks[if_block].successors.push((true_entry, EdgeKind::BranchTrue));
                        self.blocks[if_block].successors.push((false_entry, EdgeKind::BranchFalse));
                    } else {
                        let true_entry = self.build_range(current_start + 1, matching_end, next_after);
                        self.blocks[if_block].successors.push((true_entry, EdgeKind::BranchTrue));
                        self.blocks[if_block].successors.push((next_after, EdgeKind::BranchFalse));
                    }
                    return first_block.unwrap_or(if_block);
                }
                Action::Else | Action::ElseIf { .. } | Action::End => {
                    current_start += 1;
                }
                _ => {
                    let blk = self.new_block(BlockKind::StraightLine);
                    if let Some((prev, kind)) = prev_block.take() {
                        self.blocks[prev].successors.push((blk, kind));
                    }
                    if first_block.is_none() {
                        first_block = Some(blk);
                    }
                    while current_start < end {
                        match &self.actions[current_start] {
                            Action::While { .. }
                            | Action::ForGlobalVariable { .. }
                            | Action::ForPlayerVariable { .. }
                            | Action::If { .. }
                            | Action::Else
                            | Action::ElseIf { .. }
                            | Action::End => break,
                            _ => {
                                self.add_action_to_block(blk, current_start);
                                current_start += 1;
                            }
                        }
                    }
                    prev_block = Some((blk, EdgeKind::Fallthrough));
                }
            }
        }

        if let Some((prev, kind)) = prev_block {
            self.blocks[prev].successors.push((after, kind));
        }
        first_block.unwrap_or(after)
    }

    fn find_matching_end(&self, start: usize, end: usize) -> usize {
        let mut depth = 0;
        for i in start + 1..end {
            match self.actions[i] {
                Action::While { .. }
                | Action::ForGlobalVariable { .. }
                | Action::ForPlayerVariable { .. }
                | Action::If { .. } => depth += 1,
                Action::End if depth == 0 => return i,
                Action::End => depth -= 1,
                _ => {}
            }
        }
        end.saturating_sub(1)
    }

    fn find_if_branches(&self, start: usize, end: usize) -> (Option<usize>, usize) {
        let mut depth = 0;
        let mut else_idx = None;
        for i in start + 1..end {
            match self.actions[i] {
                Action::While { .. }
                | Action::ForGlobalVariable { .. }
                | Action::ForPlayerVariable { .. }
                | Action::If { .. } => depth += 1,
                Action::Else if depth == 0 && else_idx.is_none() => else_idx = Some(i),
                Action::End if depth == 0 => return (else_idx, i),
                Action::End => depth -= 1,
                _ => {}
            }
        }
        (else_idx, end.saturating_sub(1))
    }
}

fn is_action_wait(action: &Action) -> bool {
    matches!(action, Action::Call { name, .. } if name == "wait")
}

fn action_subroutine(action: &Action) -> Option<&str> {
    match action {
        Action::CallSubroutine { subroutine } => Some(subroutine),
        _ => None,
    }
}
