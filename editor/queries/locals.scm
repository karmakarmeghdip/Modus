(function_declaration) @local.scope
(block) @local.scope
(closure_expression) @local.scope

(parameter
  name: (identifier) @local.definition)

(let_statement
  name: (identifier) @local.definition)

(identifier) @local.reference
