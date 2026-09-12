use tree_sitter::Language;

extern "C" {
    fn tree_sitter_modus() -> Language;
}

pub fn language() -> Language {
    unsafe { tree_sitter_modus() }
}

pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");
pub const INDENTS_QUERY: &str = include_str!("../../queries/indents.scm");
pub const FOLDS_QUERY: &str = include_str!("../../queries/folds.scm");
pub const TEXTOBJECTS_QUERY: &str = include_str!("../../queries/textobjects.scm");
pub const LOCALS_QUERY: &str = include_str!("../../queries/locals.scm");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_load_grammar() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&language())
            .expect("Error loading Modus grammar");
    }

    #[test]
    fn test_queries_valid() {
        let lang = language();
        tree_sitter::Query::new(&lang, HIGHLIGHTS_QUERY).expect("Invalid highlights query");
        tree_sitter::Query::new(&lang, INDENTS_QUERY).expect("Invalid indents query");
        tree_sitter::Query::new(&lang, FOLDS_QUERY).expect("Invalid folds query");
        tree_sitter::Query::new(&lang, TEXTOBJECTS_QUERY).expect("Invalid textobjects query");
        tree_sitter::Query::new(&lang, LOCALS_QUERY).expect("Invalid locals query");

        // Validate Helix queries
        let helix_highlights = include_str!("../../helix/queries/modus/highlights.scm");
        let helix_indents = include_str!("../../helix/queries/modus/indents.scm");
        let helix_folds = include_str!("../../helix/queries/modus/folds.scm");
        let helix_textobjects = include_str!("../../helix/queries/modus/textobjects.scm");
        tree_sitter::Query::new(&lang, helix_highlights).expect("Invalid helix highlights query");
        tree_sitter::Query::new(&lang, helix_indents).expect("Invalid helix indents query");
        tree_sitter::Query::new(&lang, helix_folds).expect("Invalid helix folds query");
        tree_sitter::Query::new(&lang, helix_textobjects).expect("Invalid helix textobjects query");

        // Validate Zed queries
        let zed_highlights = include_str!("../../zed/languages/modus/highlights.scm");
        let zed_brackets = include_str!("../../zed/languages/modus/brackets.scm");
        let zed_outline = include_str!("../../zed/languages/modus/outline.scm");
        let zed_indents = include_str!("../../zed/languages/modus/indents.scm");
        tree_sitter::Query::new(&lang, zed_highlights).expect("Invalid zed highlights query");
        tree_sitter::Query::new(&lang, zed_brackets).expect("Invalid zed brackets query");
        tree_sitter::Query::new(&lang, zed_outline).expect("Invalid zed outline query");
        tree_sitter::Query::new(&lang, zed_indents).expect("Invalid zed indents query");
    }
}
