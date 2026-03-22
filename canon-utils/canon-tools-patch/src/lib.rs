mod parser;
mod seek_sequence;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
pub use parser::{parse_patch, ApplyPatchArgs, Hunk, ParseError, UpdateFileChunk};
use thiserror::Error;

#[derive(Debug, Error)]
#[error("{context}: {source}")]
pub struct IoError {
    context: String,
    #[source]
    source: std::io::Error,
}

#[derive(Debug, Error)]
pub enum ApplyPatchError {
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error(transparent)]
    Io(#[from] IoError),
    #[error("{0}")]
    ComputeReplacements(String),
}

#[derive(Debug, Clone)]
pub struct AffectedPaths {
    pub added: Vec<PathBuf>,
    pub modified: Vec<PathBuf>,
    pub deleted: Vec<PathBuf>,
}

/// Apply a patch string to the filesystem. Relative paths are resolved against `cwd`.
pub fn apply_patch(patch: &str, cwd: &Path) -> Result<AffectedPaths, ApplyPatchError> {
    let mut args = parse_patch(patch)?;

    // Resolve relative paths against cwd.
    for hunk in args.hunks.iter_mut() {
        match hunk {
            parser::Hunk::AddFile { path, .. }
            | parser::Hunk::DeleteFile { path }
            | parser::Hunk::UpdateFile { path, .. } => {
                if path.is_relative() {
                    *path = cwd.join(&*path);
                }
            }
        }
        if let parser::Hunk::UpdateFile { move_path, .. } = hunk {
            if let Some(dest) = move_path {
                if dest.is_relative() {
                    *dest = cwd.join(&*dest);
                }
            }
        }
    }

    apply_hunks_to_files(&args.hunks)
}

fn apply_hunks_to_files(hunks: &[Hunk]) -> Result<AffectedPaths, ApplyPatchError> {
    if hunks.is_empty() {
        return Err(ApplyPatchError::ComputeReplacements("No files were modified.".into()));
    }

    let mut added: Vec<PathBuf> = Vec::new();
    let mut modified: Vec<PathBuf> = Vec::new();
    let mut deleted: Vec<PathBuf> = Vec::new();

    for hunk in hunks {
        match hunk {
            Hunk::AddFile { path, contents } => {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent).with_context(|| format!("create parent dirs for {}", path.display())).map_err(to_io)?;
                    }
                }
                std::fs::write(path, contents).with_context(|| format!("write file {}", path.display())).map_err(to_io)?;
                added.push(path.clone());
            }
            Hunk::DeleteFile { path } => {
                std::fs::remove_file(path).with_context(|| format!("delete file {}", path.display())).map_err(to_io)?;
                deleted.push(path.clone());
            }
            Hunk::UpdateFile { path, move_path, chunks } => {
                let AppliedPatch { new_contents, .. } = derive_new_contents_from_chunks(path, chunks)?;
                if let Some(dest) = move_path {
                    if let Some(parent) = dest.parent() {
                        if !parent.as_os_str().is_empty() {
                            std::fs::create_dir_all(parent).with_context(|| format!("create parent dirs for {}", dest.display())).map_err(to_io)?;
                        }
                    }
                    std::fs::write(dest, &new_contents).with_context(|| format!("write file {}", dest.display())).map_err(to_io)?;
                    std::fs::remove_file(path).with_context(|| format!("remove original {}", path.display())).map_err(to_io)?;
                    modified.push(dest.clone());
                } else {
                    std::fs::write(path, &new_contents).with_context(|| format!("write file {}", path.display())).map_err(to_io)?;
                    modified.push(path.clone());
                }
            }
        }
    }

    Ok(AffectedPaths { added, modified, deleted })
}

struct AppliedPatch {
    new_contents: String,
}

fn derive_new_contents_from_chunks(path: &Path, chunks: &[UpdateFileChunk]) -> Result<AppliedPatch, ApplyPatchError> {
    let original_contents = std::fs::read_to_string(path).map_err(|source| {
        ApplyPatchError::Io(IoError {
            context: format!("Failed to read file to update {}", path.display()),
            source,
        })
    })?;

    let mut original_lines: Vec<String> = original_contents.split('\n').map(String::from).collect();
    if original_lines.last().is_some_and(String::is_empty) {
        original_lines.pop();
    }

    let replacements = compute_replacements(&original_lines, path, chunks)?;
    let mut new_lines = apply_replacements(original_lines, &replacements);
    if !new_lines.last().is_some_and(String::is_empty) {
        new_lines.push(String::new());
    }
    let new_contents = new_lines.join("\n");
    Ok(AppliedPatch { new_contents })
}

fn compute_replacements(
    original_lines: &[String],
    path: &Path,
    chunks: &[UpdateFileChunk],
) -> Result<Vec<(usize, usize, Vec<String>)>, ApplyPatchError> {
    let mut replacements: Vec<(usize, usize, Vec<String>)> = Vec::new();
    let mut line_index: usize = 0;

    for chunk in chunks {
        if let Some(ctx_line) = &chunk.change_context {
            if let Some(idx) = seek_sequence::seek_sequence(original_lines, std::slice::from_ref(ctx_line), line_index, false) {
                line_index = idx + 1;
            } else {
                return Err(ApplyPatchError::ComputeReplacements(format!(
                    "Failed to find context '{}' in {}",
                    ctx_line,
                    path.display()
                )));
            }
        }

        if chunk.old_lines.is_empty() {
            let insertion_idx = if original_lines.last().is_some_and(String::is_empty) {
                original_lines.len() - 1
            } else {
                original_lines.len()
            };
            replacements.push((insertion_idx, 0, chunk.new_lines.clone()));
            continue;
        }

        let mut pattern: &[String] = &chunk.old_lines;
        let mut found = seek_sequence::seek_sequence(original_lines, pattern, line_index, chunk.is_end_of_file);
        let mut new_slice: &[String] = &chunk.new_lines;

        if found.is_none() && pattern.last().is_some_and(String::is_empty) {
            pattern = &pattern[..pattern.len() - 1];
            if new_slice.last().is_some_and(String::is_empty) {
                new_slice = &new_slice[..new_slice.len() - 1];
            }
            found = seek_sequence::seek_sequence(original_lines, pattern, line_index, chunk.is_end_of_file);
        }

        if let Some(start_idx) = found {
            replacements.push((start_idx, pattern.len(), new_slice.to_vec()));
            line_index = start_idx + pattern.len();
        } else {
            return Err(ApplyPatchError::ComputeReplacements(format!(
                "Failed to find expected lines in {}:\n{}",
                path.display(),
                chunk.old_lines.join("\n"),
            )));
        }
    }

    replacements.sort_by(|(lhs_idx, _, _), (rhs_idx, _, _)| lhs_idx.cmp(rhs_idx));
    Ok(replacements)
}

fn apply_replacements(mut lines: Vec<String>, replacements: &[(usize, usize, Vec<String>)]) -> Vec<String> {
    for (start_idx, old_len, new_segment) in replacements.iter().rev() {
        let start_idx = *start_idx;
        let old_len = *old_len;
        for _ in 0..old_len {
            if start_idx < lines.len() {
                lines.remove(start_idx);
            }
        }
        for (offset, new_line) in new_segment.iter().enumerate() {
            lines.insert(start_idx + offset, new_line.clone());
        }
    }
    lines
}

fn to_io(err: anyhow::Error) -> ApplyPatchError {
    match err.downcast::<std::io::Error>() {
        Ok(ioe) => ApplyPatchError::Io(IoError { context: "I/O error".to_string(), source: ioe }),
        Err(other) => ApplyPatchError::ComputeReplacements(other.to_string()),
    }
}
