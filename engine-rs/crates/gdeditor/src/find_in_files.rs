//! Project-wide "find in files" search for the script editor.
//!
//! [`find_in_files`] scans a set of `(path, content)` scripts for a query and
//! returns the matches grouped per file, each carrying its 1-based line, 1-based
//! column, and the full line of context. Files with no match are omitted.
//! Activating a result with [`open_location`] yields the file path and line to
//! navigate to.
//!
//! Search operates over in-memory content so it stays deterministic and
//! testable; the editor supplies the open script buffers / loaded project files.

/// A single match within a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column (character offset) where the match begins.
    pub column: usize,
    /// The full text of the matched line, for display context.
    pub context: String,
}

/// All matches found within a single file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMatches {
    /// The file's path.
    pub path: String,
    /// Matches in the file, ordered by line then column.
    pub matches: Vec<Match>,
}

/// Search options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOptions {
    /// Match case exactly when true; case-insensitive when false.
    pub case_sensitive: bool,
}

/// A navigation target produced by activating a search result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenLocation {
    /// The file to open.
    pub path: String,
    /// 1-based line to place the caret on.
    pub line: usize,
    /// 1-based column to place the caret on.
    pub column: usize,
}

/// Searches every `(path, content)` pair for `query`, returning per-file match
/// groups in input order (files with no match omitted). An empty query yields no
/// results. Multiple matches on one line are all reported.
pub fn find_in_files(files: &[(&str, &str)], query: &str, opts: SearchOptions) -> Vec<FileMatches> {
    let mut out = Vec::new();
    if query.is_empty() {
        return out;
    }
    let needle = if opts.case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };
    for (path, content) in files {
        let mut matches = Vec::new();
        for (i, line) in content.lines().enumerate() {
            let hay = if opts.case_sensitive {
                line.to_string()
            } else {
                line.to_lowercase()
            };
            let mut start = 0;
            while let Some(pos) = hay[start..].find(&needle) {
                let abs = start + pos;
                let column = hay[..abs].chars().count() + 1;
                matches.push(Match {
                    line: i + 1,
                    column,
                    context: line.to_string(),
                });
                start = abs + needle.len();
            }
        }
        if !matches.is_empty() {
            out.push(FileMatches {
                path: path.to_string(),
                matches,
            });
        }
    }
    out
}

/// Resolves the navigation target for the `m`-th match of the `file`-th result
/// group. Returns `None` if either index is out of range.
pub fn open_location(results: &[FileMatches], file: usize, m: usize) -> Option<OpenLocation> {
    results.get(file).and_then(|f| {
        f.matches.get(m).map(|mm| OpenLocation {
            path: f.path.clone(),
            line: mm.line,
            column: mm.column,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-78ywg): project-wide search returns matches grouped by
    /// file with line context, and activating a result opens that file at the
    /// matched line.
    #[test]
    fn script_nav_find_in_files() {
        let files = vec![
            (
                "player.gd",
                "func move():\n    velocity = 1\n    move_and_slide()\n",
            ),
            ("enemy.gd", "func move():\n    pass\n"),
            ("ui.gd", "var label = 0\n"),
        ];
        let results = find_in_files(
            &files,
            "move",
            SearchOptions {
                case_sensitive: true,
            },
        );

        // Grouped by file, in input order; ui.gd (no match) omitted.
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].path, "player.gd");
        assert_eq!(results[1].path, "enemy.gd");

        // player.gd: "move" on line 1 ("func move():") and line 3 ("move_and_slide()").
        assert_eq!(results[0].matches.len(), 2);
        assert_eq!(results[0].matches[0].line, 1);
        assert_eq!(results[0].matches[0].column, 6); // after "func "
        assert_eq!(results[0].matches[0].context, "func move():");
        assert_eq!(results[0].matches[1].line, 3);
        assert_eq!(results[0].matches[1].column, 5); // after 4 spaces
        assert_eq!(results[0].matches[1].context, "    move_and_slide()");

        // enemy.gd: one match on line 1.
        assert_eq!(results[1].matches.len(), 1);
        assert_eq!(results[1].matches[0].line, 1);

        // Activating player.gd's second result opens that file at line 3.
        let loc = open_location(&results, 0, 1).unwrap();
        assert_eq!(
            loc,
            OpenLocation {
                path: "player.gd".to_string(),
                line: 3,
                column: 5,
            }
        );

        // Case-insensitive search matches differently-cased text.
        let ci = find_in_files(
            &[("a.gd", "Move\nmove\n")],
            "move",
            SearchOptions {
                case_sensitive: false,
            },
        );
        assert_eq!(ci.len(), 1);
        assert_eq!(ci[0].matches.len(), 2);
        assert_eq!(ci[0].matches[0].line, 1); // "Move"
        assert_eq!(ci[0].matches[1].line, 2); // "move"

        // Case-sensitive: "Move" is not matched by "move".
        let cs = find_in_files(
            &[("a.gd", "Move\nmove\n")],
            "move",
            SearchOptions {
                case_sensitive: true,
            },
        );
        assert_eq!(cs[0].matches.len(), 1);
        assert_eq!(cs[0].matches[0].line, 2);

        // Empty query yields no results.
        assert!(find_in_files(
            &files,
            "",
            SearchOptions {
                case_sensitive: true
            }
        )
        .is_empty());

        // Out-of-range activation returns None.
        assert!(open_location(&results, 9, 0).is_none());
        assert!(open_location(&results, 0, 9).is_none());
    }

    /// Multiple matches on a single line are each reported with their column.
    #[test]
    fn multiple_matches_per_line() {
        let r = find_in_files(
            &[("a.gd", "x x x\n")],
            "x",
            SearchOptions {
                case_sensitive: true,
            },
        );
        assert_eq!(r.len(), 1);
        let cols: Vec<usize> = r[0].matches.iter().map(|m| m.column).collect();
        assert_eq!(cols, vec![1, 3, 5]);
        assert!(r[0].matches.iter().all(|m| m.line == 1));
    }
}
