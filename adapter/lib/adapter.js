/**
 * OverPy AST -> Opy HIR v1 adapter.
 *
 * This module is the compatibility adapter boundary: it owns all knowledge of
 * how the pinned OverPy frontend's parsed AST maps onto the Wright-owned
 * `wright/opy-hir` protocol (docs/hir/opy-hir-v1.md). It never leaks an
 * OverPy type into the protocol, and it refuses to map constructs that are
 * outside the v0.1 corpus boundary.
 */

import overpy from "overpy";
import { AdapterError, unsupported } from "./errors.js";

/**
 * Function names the frontend recognizes as callable. Member functions keep
 * their leading dot (`.append`) and are mapped by name; everything in this
 * set is emitted as a `call` node.
 */
const KNOWN_FUNCTIONS = new Set([
  ...Object.keys(overpy.actionKw),
  ...Object.keys(overpy.valueFuncKw),
  ...Object.keys(overpy.opyFuncs),
  ...Object.keys(overpy.opyMacros),
]);

/** Enumerated value domains the protocol models as `enum` nodes. */
const ENUM_TYPES = new Set(Object.keys(overpy.constantValues));

const BINARY_OPS = {
  __add__: "+",
  __subtract__: "-",
  __multiply__: "*",
  __divide__: "/",
  __modulo__: "%",
  __raiseToPower__: "**",
  __equals__: "==",
  __inequals__: "!=",
  __lessThan__: "<",
  __lessThanOrEquals__: "<=",
  __greaterThan__: ">",
  __greaterThanOrEquals__: ">=",
  __and__: "and",
  __or__: "or",
};

const UNARY_OPS = {
  __not__: "not",
  __negate__: "-",
};

/** Enum wrappers: `__team__(ALL)`, `__color__(WHITE)`, and friends. */
const ENUM_WRAPPERS = new Set([
  "__team__",
  "__color__",
  "__hero__",
  "__map__",
  "__gamemode__",
  "__button__",
]);

/**
 * Convert parsed frontend state into an Opy HIR v1 program object.
 *
 * @param {object} options
 * @param {object[]} options.astRules Parsed top-level AST nodes (`__rule__`,
 *   `__def__`, and frontend plumbing rules).
 * @param {object} options.compiler The frontend compiler state (variables,
 *   subroutines, constants, macros, initializers, defines).
 * @param {import("./span.js").SpanBuilder} options.spans
 * @param {object} options.generator `{ name, version, frontend }`
 * @returns {object} The HIR program object.
 */
export function convertProgram({ astRules, compiler, spans, generator }) {
  const mapper = createMapper(compiler.astMacros, spans);
  const declarations = [];
  const rules = [];

  for (const variable of compiler.globalVariables) {
    declarations.push({
      kind: "globalVariable",
      name: variable.name,
      index: variable.index >= 0 ? variable.index : null,
      span: spans.fromFileStackMember(variable.fileStack?.[0]),
      initializer: null,
    });
  }
  for (const variable of compiler.playerVariables) {
    declarations.push({
      kind: "playerVariable",
      name: variable.name,
      index: variable.index >= 0 ? variable.index : null,
      span: spans.fromFileStackMember(variable.fileStack?.[0]),
      initializer: null,
    });
  }
  for (const subroutine of compiler.subroutines) {
    if (subroutine.isFromDefStatement) {
      continue; // represented by its `subroutineDef` in `rules`
    }
    declarations.push({
      kind: "subroutine",
      name: subroutine.name,
      index: subroutine.index >= 0 ? subroutine.index : null,
      span: spans.fromFileStackMember(subroutine.fileStack?.[0]),
    });
  }
  for (const name of Object.keys(compiler.astConstants)) {
    const constant = compiler.astConstants[name];
    declarations.push({
      kind: "constant",
      name: constant.name,
      span: spans.fromFileStackMember(constant.value?.fileStack?.[0]),
      value: mapper.expr(constant.value),
    });
  }
  for (const name of Object.keys(compiler.astMacros)) {
    const macro = compiler.astMacros[name];
    declarations.push({
      kind: "macro",
      name: macro.name,
      args: macro.args.map((arg) => arg.name),
      span: spans.fromFileStackMember(macro.lines?.[0]?.fileStack?.[0]),
      body: mapper.statements(macro.lines),
    });
  }

  for (const ast of astRules) {
    if (ast.name === "__rule__") {
      rules.push(mapRule(ast, mapper));
    } else if (ast.name === "__def__") {
      rules.push(mapSubroutineDef(ast, mapper));
    } else if (ast.name === "__pushRulePrefixStack__" || ast.name === "__popRulePrefixStack__") {
      // Frontend plumbing emitted around includes; not part of the program.
      continue;
    } else if (ast.name === "__settings__") {
      throw unsupported(
        "custom game settings blocks are outside the Opy HIR v1 corpus boundary",
        mapper.span(ast),
      );
    } else {
      throw unsupported(
        `frontend node '${ast.name}' is outside the Opy HIR v1 corpus boundary`,
        mapper.span(ast),
      );
    }
  }

  attachInitializers(declarations, compiler, mapper);

  const defines = (compiler.macros ?? []).map((macro) => ({
    name: macro.name,
    isFunction: Boolean(macro.isFunction),
    span: mapper.spanFromFileStack(macro.fileStack),
  }));

  // Register every imported file so spans and diagnostics can reference it,
  // even when no parsed node happens to carry a span inside it.
  for (const imported of compiler.importedFiles ?? []) {
    if (typeof imported !== "string" || imported.endsWith("/")) {
      continue;
    }
    const name = imported.split("/").filter(Boolean).pop();
    if (name) {
      spans.fileId(name);
    }
  }

  return {
    protocol: { name: "wright/opy-hir", version: "1.0.0" },
    generator,
    files: spans.files(),
    defines,
    declarations,
    rules,
  };
}

/**
 * Create the mapper functions for one conversion, closing over the frontend
 * macro table.
 *
 * @param {object} astMacros
 * @param {import("./span.js").SpanBuilder} spans
 */
function createMapper(astMacros, spans) {
  return {
    /** Span of a node from its innermost file-stack entry. */
    span(node) {
      return spans.fromFileStackMember(node?.fileStack?.[0]);
    },
    spanFromFileStack(fileStack) {
      return spans.fromFileStackMember(fileStack?.[0]);
    },
    expr(node) {
      return mapExpr(node, { astMacros, spans });
    },
    statements(nodes) {
      return mapStatements(nodes, { astMacros, spans });
    },
  };
}

/**
 * Attach frontend-generated variable initializers to their declarations.
 *
 * @param {object[]} declarations
 * @param {object} compiler
 * @param {object} mapper
 */
function attachInitializers(declarations, compiler, mapper) {
  const byName = new Map(declarations.map((declaration) => [declaration.name, declaration]));
  for (const directive of compiler.globalInitDirectives ?? []) {
    const target = directive.args?.[0];
    const declaration = target && target.name === "__globalVar__" ? byName.get(target.args?.[0]?.name) : undefined;
    if (declaration) {
      declaration.initializer = mapper.expr(directive.args[1]);
    }
  }
  for (const directive of compiler.playerInitDirectives ?? []) {
    const target = directive.args?.[0];
    const declaration = target && target.name === "__playerVar__" ? byName.get(target.args?.[1]?.name) : undefined;
    if (declaration) {
      declaration.initializer = mapper.expr(directive.args[1]);
    }
  }
}

/**
 * Map a `__rule__` node to a HIR rule.
 *
 * @param {object} ast
 * @param {object} mapper
 * @returns {object}
 */
function mapRule(ast, mapper) {
  const attributes = ast.ruleAttributes ?? {};
  const span = mapper.span(ast);
  let event = null;
  const conditions = [];
  let disabled = false;

  const actionNodes = [];
  for (const child of ast.children) {
    if (child.name === "@Event") {
      event = mapEvent(child, mapper);
    } else if (child.name === "@Condition") {
      if (child.args.length !== 1) {
        throw unsupported("multi-argument @Condition is outside the Opy HIR v1 corpus boundary", mapper.span(child));
      }
      conditions.push(mapper.expr(child.args[0]));
    } else if (child.name === "@Disabled") {
      disabled = true;
    } else if (child.name.startsWith("@")) {
      throw unsupported(
        `annotation '${child.name}' is outside the Opy HIR v1 corpus boundary`,
        mapper.span(child),
      );
    } else {
      actionNodes.push(child);
    }
  }

  if (event === null) {
    event = { name: "global", args: [], span };
  }

  return {
    name: typeof attributes.name === "string" ? attributes.name : "",
    span,
    disabled,
    event,
    conditions,
    actions: mapper.statements(actionNodes),
  };
}

/**
 * Map a rule's `@Event` annotation to a HIR event.
 *
 * @param {object} ast
 * @param {object} mapper
 * @returns {object}
 */
function mapEvent(ast, mapper) {
  const first = ast.args[0];
  if (!first) {
    throw unsupported("empty @Event annotation is outside the Opy HIR v1 corpus boundary", mapper.span(ast));
  }
  return {
    name: first.name,
    args: ast.args.slice(1).map((arg) => ({
      kind: "string",
      value: arg.name,
      span: mapper.span(arg),
    })),
    span: mapper.span(ast),
  };
}

/**
 * Map a `__def__` node to a HIR `subroutineDef` rule entry.
 *
 * @param {object} ast
 * @param {object} mapper
 * @returns {object}
 */
function mapSubroutineDef(ast, mapper) {
  const name = ast.ruleAttributes?.subroutineName;
  if (typeof name !== "string") {
    throw unsupported("malformed subroutine definition without a name", mapper.span(ast));
  }
  return {
    kind: "subroutineDef",
    name,
    span: mapper.span(ast),
    body: mapper.statements(ast.children),
  };
}

/**
 * Map a list of frontend statement ASTs to HIR statements, grouping
 * `__if__`/`__elif__`/`__else__` chains into single `if` nodes.
 *
 * @param {object[]} nodes
 * @param {{ astMacros: object, spans: import("./span.js").SpanBuilder }} context
 * @returns {object[]}
 */
function mapStatements(nodes, context) {
  const result = [];
  let i = 0;
  while (i < nodes.length) {
    const node = nodes[i];
    if (node.name === "__if__" || node.name === "__elif__" || node.name === "__else__") {
      const branches = [];
      while (i < nodes.length && (nodes[i].name === "__if__" || nodes[i].name === "__elif__")) {
        branches.push({
          condition: mapExpr(nodes[i].args[0], context),
          body: mapStatements(nodes[i].children, context),
        });
        i++;
      }
      let elseBody = null;
      if (i < nodes.length && nodes[i].name === "__else__") {
        elseBody = mapStatements(nodes[i].children, context);
        i++;
      }
      result.push({
        kind: "if",
        branches,
        else: elseBody,
        span: context.spans.fromFileStackMember(node.fileStack?.[0]),
      });
      continue;
    }
    result.push(mapStmt(node, context));
    i++;
  }
  return result;
}

/**
 * Map one non-control-flow statement AST to a HIR statement.
 *
 * @param {object} node
 * @param {{ astMacros: object, spans: import("./span.js").SpanBuilder }} context
 * @returns {object}
 */
function mapStmt(node, context) {
  const span = () => context.spans.fromFileStackMember(node.fileStack?.[0]);
  switch (node.name) {
    case "__assignTo__":
      return { kind: "assign", target: mapExpr(node.args[0], context), value: mapExpr(node.args[1], context), span: span() };
    case "__for__": {
      const header = node.args[0];
      if (!header || header.name !== "__arrayContains__") {
        throw unsupported("for-loop header shape is outside the Opy HIR v1 corpus boundary", span());
      }
      return {
        kind: "for",
        variable: mapExpr(header.args[1], context),
        iterable: mapExpr(header.args[0], context),
        body: mapStatements(node.children, context),
        span: span(),
      };
    }
    case "__while__":
      return { kind: "while", condition: mapExpr(node.args[0], context), body: mapStatements(node.children, context), span: span() };
    case "__callSubroutine__":
      return { kind: "callSubroutine", name: node.args[0].name, span: span() };
    case "pass":
      return { kind: "pass", span: span() };
    default:
      return { kind: "expr", expr: mapExpr(node, context), span: span() };
  }
}

/**
 * Map a frontend expression AST to a HIR expression.
 *
 * @param {object} node
 * @param {{ astMacros: object, spans: import("./span.js").SpanBuilder }} context
 * @returns {object}
 */
function mapExpr(node, context) {
  const span = () => context.spans.fromFileStackMember(node.fileStack?.[0]);

  switch (node.name) {
    case "__number__": {
      const literal = node.args[0];
      return {
        kind: "number",
        value: literal.numValue !== undefined ? literal.numValue : Number(literal.name),
        text: literal.name,
        span: span(),
      };
    }
    case "__customString__": {
      const text = node.args[0].name;
      const args = node.args.slice(1).map((arg) => mapExpr(arg, context));
      if (args.length === 0) {
        return { kind: "string", value: text, span: span() };
      }
      return { kind: "format", text, args, span: span() };
    }
    case "__array__":
      return { kind: "array", elements: node.args.map((arg) => mapExpr(arg, context)), span: span() };
    case "vect":
      return {
        kind: "vector",
        x: mapExpr(node.args[0], context),
        y: mapExpr(node.args[1], context),
        z: mapExpr(node.args[2], context),
        span: span(),
      };
    case "__globalVar__":
      return { kind: "globalVar", name: node.args[0].name, span: span() };
    case "__playerVar__":
      return { kind: "playerVar", player: mapExpr(node.args[0], context), name: node.args[1].name, span: span() };
    case "eventPlayer":
      return { kind: "eventPlayer", span: span() };
    case "null":
      return { kind: "null", span: span() };
    case "true":
      return { kind: "bool", value: true, span: span() };
    case "false":
      return { kind: "bool", value: false, span: span() };
    case "__valueInArray__":
      return { kind: "index", array: mapExpr(node.args[0], context), index: mapExpr(node.args[1], context), span: span() };
    default:
      break;
  }

  if (node.name in BINARY_OPS) {
    return {
      kind: "binary",
      op: BINARY_OPS[node.name],
      left: mapExpr(node.args[0], context),
      right: mapExpr(node.args[1], context),
      span: span(),
    };
  }
  if (node.name in UNARY_OPS) {
    return { kind: "unary", op: UNARY_OPS[node.name], operand: mapExpr(node.args[0], context), span: span() };
  }
  if (ENUM_WRAPPERS.has(node.name)) {
    return { kind: "enum", type: node.type, value: node.args[0].name, span: span() };
  }
  if (node.name.startsWith(".")) {
    return {
      kind: "receiverCall",
      receiver: mapExpr(node.args[0], context),
      name: node.name.slice(1),
      args: node.args.slice(1).map((arg) => mapExpr(arg, context)),
      span: span(),
    };
  }
  if (node.name.startsWith("$")) {
    return { kind: "macroParam", name: node.name.slice(1), span: span() };
  }
  if (node.name in context.astMacros) {
    return { kind: "macroCall", name: node.name, args: node.args.map((arg) => mapExpr(arg, context)), span: span() };
  }
  if (KNOWN_FUNCTIONS.has(node.name)) {
    return { kind: "call", name: node.name, args: node.args.map((arg) => mapExpr(arg, context)), span: span() };
  }
  return mapBareLiteral(node, context);
}

/**
 * Map a bare literal/symbol node (no args, not a function) by its frontend
 * type, or reject it explicitly.
 *
 * @param {object} node
 * @param {{ astMacros: object, spans: import("./span.js").SpanBuilder }} context
 * @returns {object}
 */
function mapBareLiteral(node, context) {
  const span = () => context.spans.fromFileStackMember(node.fileStack?.[0]);
  if (node.args.length > 0 || node.children.length > 0) {
    throw unsupported(`construct '${node.name}' is outside the Opy HIR v1 corpus boundary`, span());
  }
  const type = node.type;
  if (typeof type !== "string") {
    throw unsupported(
      `construct '${node.name}' (type ${JSON.stringify(type)}) is outside the Opy HIR v1 corpus boundary`,
      span(),
    );
  }
  if (type === "StringLiteral" || type === "CustomStringLiteral") {
    return { kind: "string", value: node.name, span: span() };
  }
  if (type === "BoolLiteral") {
    return { kind: "bool", value: node.name === "true", span: span() };
  }
  if (/Literal$/.test(type)) {
    // Numeric literal types: IntLiteral, UnsignedIntLiteral, FloatLiteral, ...
    return { kind: "number", value: node.numValue !== undefined ? node.numValue : Number(node.name), text: node.name, span: span() };
  }
  if (ENUM_TYPES.has(type)) {
    return { kind: "enum", type, value: node.name, span: span() };
  }
  if (type === "GlobalVariable") {
    return { kind: "globalVar", name: node.name, span: span() };
  }
  throw unsupported(`literal '${node.name}' of type '${type}' is outside the Opy HIR v1 corpus boundary`, span());
}

export { AdapterError };
