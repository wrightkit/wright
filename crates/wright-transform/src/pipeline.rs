//! The pass pipeline: ordering, profiles, metrics, and validation.

use wright_ir::wir;

use crate::fold_constants::FoldConstants;
use crate::profile::Profile;
use crate::synthesize_initializers::SynthesizeInitializers;

/// A transformation pass over validated WIR.
pub trait Pass {
    /// The stable pass name (reported in metrics and regression fixtures).
    fn name(&self) -> &'static str;

    /// Run the pass over the program and return its statistics.
    fn run(&self, program: &mut wir::Program) -> PassStats;
}

/// Statistics for one pass run.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PassStats {
    /// The stable pass name.
    pub pass: String,
    /// The number of nodes this pass rewrote.
    pub changed: usize,
    /// Total value/action nodes before the pass.
    pub nodes_before: usize,
    /// Total value/action nodes after the pass.
    pub nodes_after: usize,
}

/// One recorded pass run.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PassResult {
    pub stats: PassStats,
}

/// Run the pipeline for a profile over a validated program.
///
/// The program is validated before and after the pipeline; `Err` is returned
/// if the input was invalid or a pass left it invalid (a pass bug).
pub fn run(
    program: &mut wir::Program,
    profile: Profile,
) -> Result<Vec<PassResult>, wright_ir::error::IrError> {
    program.validate()?;
    let passes: Vec<Box<dyn Pass>> = match profile {
        Profile::Off => Vec::new(),
        Profile::Compat | Profile::Aggressive => {
            vec![Box::new(FoldConstants), Box::new(SynthesizeInitializers)]
        }
    };
    let mut results = Vec::new();
    for pass in passes {
        let stats = pass.run(program);
        program.validate()?;
        results.push(PassResult { stats });
    }
    Ok(results)
}
