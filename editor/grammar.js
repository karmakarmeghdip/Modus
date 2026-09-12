/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

const PREC = {
  closure: 1,
  or: 2,
  and: 3,
  equality: 4,
  comparison: 5,
  additive: 6,
  multiplicative: 7,
  unary: 8,
  call: 9,
  field: 10,
};

module.exports = grammar({
  name: 'modus',

  extras: $ => [
    /\s/,
    $.line_comment,
    $.block_comment,
  ],

  word: $ => $.identifier,

  conflicts: $ => [
    [$._type, $.function_type],
    [$.type_param, $.parameter],
  ],

  rules: {
    source_file: $ => repeat($._top_level_item),

    _top_level_item: $ => choice(
      $.library_declaration,
      $.import_declaration,
      $.export_declaration,
      $._declaration,
      $._statement,
    ),

    // Comments
    line_comment: _ => token(seq('//', /[^\n]*/)),
    block_comment: _ => token(seq('/*', /[^*]*\*+([^/*][^*]*\*+)*/, '/')),

    // Identifiers
    identifier: _ => /[a-zA-Z_][a-zA-Z0-9_]*/,

    // Primitives and Literals
    integer_literal: _ => /[0-9]+/,
    float_literal: _ => /[0-9]+\.[0-9]+/,
    boolean_literal: _ => choice('true', 'false'),
    string_literal: $ => seq(
      '"',
      repeat(choice(
        token.immediate(prec(1, /[^"\\\n]+/)),
        $.escape_sequence,
      )),
      '"',
    ),
    escape_sequence: _ => token.immediate(seq('\\', /["\\/bfnrt]/)),
    unit_literal: _ => seq('(', ')'),

    // Primitive Types
    primitive_type: _ => choice(
      'u8', 'u16', 'u32', 'u64',
      'i8', 'i16', 'i32', 'i64',
      'f32', 'f64',
      'bool', 'String', 'void',
    ),

    // Types
    _type: $ => choice(
      $.primitive_type,
      $.array_type,
      $.record_type,
      $.function_type,
      $.generic_type,
      $.path_type,
      $.tuple_type,
      $.identifier,
    ),

    array_type: $ => seq('[', field('element', $._type), ']'),

    record_type: $ => seq(
      '{',
      commaSep($.record_type_field),
      optional(','),
      '}',
    ),

    record_type_field: $ => seq(
      field('name', $.identifier),
      ':',
      field('type', $._type),
    ),

    path_type: $ => prec(1, seq(
      $.identifier,
      repeat1(seq('.', $.identifier)),
    )),

    tuple_type: $ => seq(
      '(',
      commaSep($._type),
      optional(','),
      ')',
    ),

    generic_type: $ => prec(2, seq(
      field('name', choice($.path_type, $.identifier)),
      field('arguments', $.tuple_type),
    )),

    function_type: $ => prec.right(seq(
      field('parameters', $.tuple_type),
      '=>',
      field('return_type', $._type),
    )),

    // Type Parameters (Generics on declarations)
    type_parameters: $ => seq(
      '(',
      commaSep($.type_param),
      optional(','),
      ')',
    ),

    type_param: $ => seq(
      field('name', $.identifier),
      optional(seq(':', field('bound', $._type))),
    ),

    // Parameter List for Functions and Closures
    parameter_list: $ => seq(
      '(',
      commaSep($.parameter),
      optional(','),
      ')',
    ),

    parameter: $ => seq(
      field('name', $.identifier),
      ':',
      field('type', $._type),
    ),

    // Declarations
    _declaration: $ => choice(
      $.function_declaration,
      $.type_declaration,
      $.trait_declaration,
      $.impl_declaration,
      $.extern_declaration,
    ),

    library_declaration: $ => seq(
      'library',
      field('name', $.string_literal),
      ';',
    ),

    import_declaration: $ => seq(
      'import',
      choice(
        seq($.import_clause, 'from', field('source', $.string_literal)),
        field('source', $.string_literal),
      ),
      ';',
    ),

    import_clause: $ => choice(
      $.named_imports,
      $.namespace_import,
    ),

    named_imports: $ => seq(
      '{',
      commaSep($.import_specifier),
      optional(','),
      '}',
    ),

    import_specifier: $ => seq(
      field('name', $.identifier),
      optional(seq('as', field('alias', $.identifier))),
    ),

    namespace_import: $ => seq(
      '*',
      'as',
      field('alias', $.identifier),
    ),

    export_declaration: $ => seq(
      'export',
      choice(
        $._declaration,
        seq(
          choice(
            seq($.named_exports, optional(seq('from', field('source', $.string_literal)))),
            seq($.all_export, 'from', field('source', $.string_literal)),
          ),
          ';',
        ),
      ),
    ),

    named_exports: $ => seq(
      '{',
      commaSep($.export_specifier),
      optional(','),
      '}',
    ),

    export_specifier: $ => seq(
      field('name', $.identifier),
      optional(seq('as', field('alias', $.identifier))),
    ),

    all_export: $ => seq(
      '*',
      optional(seq('as', field('alias', $.identifier))),
    ),

    function_declaration: $ => seq(
      'function',
      field('name', $.identifier),
      optional(field('type_parameters', $.type_parameters)),
      field('parameters', $.parameter_list),
      optional(seq(':', field('return_type', $._type))),
      choice(
        field('body', $.block),
        seq('=>', field('body', $._expression), ';'),
        ';',
      ),
    ),

    type_declaration: $ => seq(
      'type',
      field('name', $.identifier),
      optional(field('type_parameters', $.type_parameters)),
      '=',
      field('definition', choice(
        $.union_type,
        $._type,
      )),
      ';',
    ),

    union_type: $ => choice(
      seq(optional('|'), $.variant_declaration, repeat1(seq('|', $.variant_declaration))),
      seq('|', $.variant_declaration),
    ),

    variant_declaration: $ => seq(
      field('name', $.identifier),
      optional(field('parameters', $.tuple_type)),
    ),

    trait_declaration: $ => seq(
      'trait',
      field('name', $.identifier),
      field('type_parameters', $.type_parameters),
      '{',
      repeat($.trait_member),
      '}',
    ),

    trait_member: $ => seq(
      'function',
      field('name', $.identifier),
      field('parameters', $.parameter_list),
      ':',
      field('return_type', $._type),
      ';',
    ),

    impl_declaration: $ => seq(
      'impl',
      field('trait', $.identifier),
      'for',
      field('target', $._type),
      '{',
      repeat($.function_declaration),
      '}',
    ),

    extern_declaration: $ => seq(
      'extern',
      optional(field('abi', $.string_literal)),
      choice(
        seq('{', repeat($.function_declaration), '}'),
        $.function_declaration,
      ),
    ),

    // Statements
    _statement: $ => choice(
      $.let_statement,
      $.return_statement,
      $.expression_statement,
    ),

    let_statement: $ => seq(
      'let',
      field('name', $.identifier),
      optional(seq(':', field('type', $._type))),
      '=',
      field('value', $._expression),
      ';',
    ),

    return_statement: $ => seq(
      'return',
      optional(field('value', $._expression)),
      ';',
    ),

    expression_statement: $ => choice(
      seq($._expression, ';'),
      prec(-1, choice($.if_expression, $.match_expression)),
    ),

    block: $ => seq(
      '{',
      repeat($._statement),
      optional(prec(-1, $._expression)),
      '}',
    ),

    // Expressions
    _expression: $ => choice(
      $.identifier,
      $.integer_literal,
      $.float_literal,
      $.string_literal,
      $.boolean_literal,
      $.unit_literal,
      $.array_expression,
      $.record_expression,
      $.closure_expression,
      $.if_expression,
      $.match_expression,
      $.block,
      $.parenthesized_expression,
      $.call_expression,
      $.field_expression,
      $.index_expression,
      $.unary_expression,
      $.binary_expression,
    ),

    parenthesized_expression: $ => seq('(', $._expression, ')'),

    array_expression: $ => seq(
      '[',
      commaSep($._expression),
      optional(','),
      ']',
    ),

    record_expression: $ => seq(
      '{',
      choice(
        seq($.spread_element, repeat(seq(',', $.record_field))),
        commaSep1($.record_field),
      ),
      optional(','),
      '}',
    ),

    spread_element: $ => seq('...', field('value', $._expression)),

    record_field: $ => seq(
      field('name', $.identifier),
      ':',
      field('value', $._expression),
    ),

    closure_expression: $ => prec.right(PREC.closure, seq(
      field('parameters', $.parameter_list),
      optional(seq(':', field('return_type', $._type))),
      '=>',
      field('body', $._expression),
    )),

    if_expression: $ => prec.right(seq(
      'if',
      field('condition', seq('(', $._expression, ')')),
      field('consequence', $.block),
      optional(seq(
        'else',
        field('alternative', choice(
          $.if_expression,
          $.block,
        )),
      )),
    )),

    match_expression: $ => prec(PREC.call + 1, seq(
      'match',
      field('value', $._expression),
      '{',
      commaSep($.match_arm),
      optional(','),
      '}',
    )),

    match_arm: $ => seq(
      field('pattern', $._pattern),
      '=>',
      field('body', $._expression),
    ),

    call_expression: $ => prec(PREC.call, seq(
      field('function', $._expression),
      field('arguments', $.argument_list),
    )),

    argument_list: $ => seq(
      '(',
      commaSep($._expression),
      optional(','),
      ')',
    ),

    field_expression: $ => prec(PREC.field, seq(
      field('argument', $._expression),
      '.',
      field('field', $.identifier),
    )),

    index_expression: $ => prec(PREC.call, seq(
      field('argument', $._expression),
      '[',
      field('index', $._expression),
      ']',
    )),

    unary_expression: $ => prec(PREC.unary, seq(
      field('operator', choice('!', '-', 'perform', 'check')),
      field('argument', $._expression),
    )),

    binary_expression: $ => {
      const table = [
        [PREC.or, '||'],
        [PREC.and, '&&'],
        [PREC.equality, '=='],
        [PREC.equality, '!='],
        [PREC.comparison, '<'],
        [PREC.comparison, '<='],
        [PREC.comparison, '>'],
        [PREC.comparison, '>='],
        [PREC.additive, '+'],
        [PREC.additive, '-'],
        [PREC.multiplicative, '*'],
        [PREC.multiplicative, '/'],
        [PREC.multiplicative, '%'],
      ];

      return choice(...table.map(([precedence, operator]) =>
        prec.left(precedence, seq(
          field('left', $._expression),
          field('operator', operator),
          field('right', $._expression),
        ))
      ));
    },

    // Patterns
    _pattern: $ => choice(
      $.wildcard_pattern,
      $.literal_pattern,
      $.record_pattern,
      $.tuple_pattern,
      $.parenthesized_pattern,
      $.variant_pattern,
      $.identifier,
    ),

    wildcard_pattern: _ => '_',

    literal_pattern: $ => choice(
      $.integer_literal,
      $.float_literal,
      $.string_literal,
      $.boolean_literal,
      $.unit_literal,
      seq('-', choice($.integer_literal, $.float_literal)),
    ),

    record_pattern: $ => seq(
      '{',
      commaSep($.record_pattern_field),
      optional(','),
      '}',
    ),

    record_pattern_field: $ => seq(
      field('name', $.identifier),
      optional(seq(':', field('pattern', $._pattern))),
    ),

    tuple_pattern: $ => seq(
      '(',
      $._pattern,
      ',',
      commaSep($._pattern),
      optional(','),
      ')',
    ),

    parenthesized_pattern: $ => seq('(', $._pattern, ')'),

    variant_pattern: $ => prec(1, choice(
      // Option.Some(x) or Option.None
      seq(
        field('type', $.identifier),
        '.',
        field('variant', $.identifier),
        optional(field('patterns', seq(
          '(',
          commaSep($._pattern),
          optional(','),
          ')',
        ))),
      ),
      // Some(x)
      seq(
        field('variant', $.identifier),
        field('patterns', seq(
          '(',
          commaSep($._pattern),
          optional(','),
          ')',
        )),
      ),
    )),
  },
});

function commaSep(rule) {
  return optional(commaSep1(rule));
}

function commaSep1(rule) {
  return seq(rule, repeat(seq(',', rule)));
}
