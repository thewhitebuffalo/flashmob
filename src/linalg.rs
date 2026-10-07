use faer::linalg::solvers::Solve;
use faer::sparse::linalg::solvers::Lu;
use faer::sparse::{SparseColMat, Triplet};

/// Diagnostic threshold for the RHS-relative residual; solves use backward error.
#[cfg(test)]
pub const MAX_RELATIVE_RESIDUAL: f64 = 1e-6;

/// Largest accepted normwise backward error after one refinement step.
pub const MAX_BACKWARD_ERROR: f64 = 1e-10;

/// Owned factorization, reusable for any right-hand side of the same matrix.
pub struct SparseFactor {
    n: usize,
    lu: Option<Lu<usize, f64>>,
    /// Summed nonzero entries retained for refinement and backward error.
    entries: Vec<(usize, usize, f64)>,
}

impl SparseFactor {
    pub fn new(n: usize, triplets: &[(usize, usize, f64)]) -> Result<Self, String> {
        if triplets.iter().any(|&(r, c, v)| r >= n || c >= n || !v.is_finite()) {
            return Err("matrix or right-hand side contains invalid indices or non-finite values".into());
        }
        if n == 0 { return Ok(Self { n, lu: None, entries: Vec::new() }); }
        let trips: Vec<Triplet<usize, usize, f64>> = triplets.iter().copied()
            .filter(|(_, _, v)| *v != 0.0)
            .map(|(r, c, v)| Triplet::new(r, c, v)).collect();
        if trips.is_empty() { return Err("the network matrix is empty".into()); }
        let mat = SparseColMat::<usize, f64>::try_new_from_triplets(n, n, &trips)
            .map_err(|err| format!("could not build the network matrix: {err}"))?;
        let lu = mat.sp_lu()
            .map_err(|err| format!("the network matrix is singular: {err}"))?;
        let mut summed = std::collections::BTreeMap::new();
        for t in &trips { *summed.entry((t.row, t.col)).or_insert(0.0) += t.val; }
        let entries = summed.into_iter().map(|((r, c), v)| (r, c, v)).collect();
        Ok(Self { n, lu: Some(lu), entries })
    }

    pub fn solve(&self, b: &[f64]) -> Result<Vec<f64>, String> {
        if b.len() != self.n {
            return Err("right-hand side length does not match the matrix".into());
        }
        if b.iter().any(|v| !v.is_finite()) {
            return Err("matrix or right-hand side contains invalid indices or non-finite values".into());
        }
        let Some(lu) = &self.lu else { return Ok(Vec::new()); };
        let rhs = faer::col::Col::from_fn(self.n, |i| b[i]);
        let x = lu.solve(&rhs);
        let mut out: Vec<f64> = (0..self.n).map(|i| x[i]).collect();
        let mut ax = vec![0.0; self.n];
        for &(row, col, v) in &self.entries { ax[row] += v * out[col]; }
        let correction = lu.solve(&faer::col::Col::from_fn(self.n, |i| b[i] - ax[i]));
        for i in 0..self.n { out[i] += correction[i]; }
        if out.iter().any(|v| !v.is_finite()) { return Err("non-finite linear solution".into()); }
        let eta = backward_error(self.n, &self.entries, &out, b);
        if !(eta <= MAX_BACKWARD_ERROR) {
            return Err(format!("the network matrix is numerically singular (normwise backward error {eta:.1e})"));
        }
        Ok(out)
    }
}

/// Infinity-norm backward error; entries are the matrix's summed nonzeros.
pub fn backward_error(n: usize, entries: &[(usize, usize, f64)], x: &[f64], b: &[f64]) -> f64 {
    let mut ax = vec![0.0; n];
    let mut rows = vec![0.0; n];
    for &(r, c, v) in entries { ax[r] += v * x[c]; rows[r] += v.abs(); }
    if ax.iter().chain(x).chain(b).any(|v| !v.is_finite()) { return f64::INFINITY; }
    let residual = ax.iter().zip(b).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
    let a_norm = rows.into_iter().fold(0.0, f64::max);
    let x_norm = x.iter().map(|v| v.abs()).fold(0.0, f64::max);
    let b_norm = b.iter().map(|v| v.abs()).fold(0.0, f64::max);
    let den = a_norm * x_norm + b_norm;
    if den == 0.0 { if residual == 0.0 { 0.0 } else { f64::INFINITY } } else { residual / den }
}

/// Relative infinity-norm residual `max|Ax-b| / max|b|`. Duplicate entries are
/// summed. A zero right-hand side gives zero only for an exact zero residual.
#[cfg(test)]
pub fn relative_residual(n: usize, entries: &[(usize, usize, f64)], x: &[f64], b: &[f64]) -> f64 {
    let mut ax = vec![0.0; n];
    for &(r, c, v) in entries { ax[r] += v * x[c]; }
    let num = ax.iter().zip(b).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
    let den = b.iter().map(|v| v.abs()).fold(0.0, f64::max);
    if den == 0.0 { if num == 0.0 { 0.0 } else { f64::INFINITY } } else { num / den }
}

/// Solve `A x = b` with a sparse LU factorization. Duplicate triplets are summed.
pub fn solve_sparse(n: usize, triplets: &[(usize, usize, f64)], b: &[f64]) -> Result<Vec<f64>, String> {
    if b.len() != n { return Err("right-hand side length does not match the matrix".into()); }
    if b.iter().any(|v| !v.is_finite()) {
        return Err("matrix or right-hand side contains invalid indices or non-finite values".into());
    }
    SparseFactor::new(n, triplets)?.solve(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reusable_factor_validates_rhs_and_sums_duplicate_entries() {
        let factor = SparseFactor::new(2, &[(0, 0, 1.0), (0, 0, 1.0), (1, 1, 4.0)]).unwrap();
        assert_eq!(factor.solve(&[2.0, 8.0]).unwrap(), vec![1.0, 2.0]);
        assert!(factor.solve(&[1.0]).is_err());
        assert!(factor.solve(&[f64::NAN, 0.0]).is_err());
        assert_eq!(factor.solve(&[6.0, 4.0]).unwrap(), vec![3.0, 1.0]);
        assert!(SparseFactor::new(1, &[(1, 0, 1.0)]).is_err());
        assert!(SparseFactor::new(1, &[(0, 0, f64::INFINITY)]).is_err());
        assert!(SparseFactor::new(0, &[]).unwrap().solve(&[]).unwrap().is_empty());
    }

    #[test]
    fn residual_check_accepts_good_solutions_and_flags_bad_ones() {
        let entries = [(0, 0, 2.0), (0, 1, 1.0), (1, 0, 1.0), (1, 1, 3.0)];
        let b = [3.0, 4.0];
        assert!(relative_residual(2, &entries, &[1.0, 1.0], &b) < 1e-15);
        assert!(relative_residual(2, &entries, &[1.0, 1.1], &b) > MAX_RELATIVE_RESIDUAL);
        assert!(backward_error(2, &entries, &[1.0, 1.0], &b) < 1e-15);
        assert!(backward_error(2, &entries, &[1.0, 1.1], &b) > MAX_BACKWARD_ERROR);
        assert_eq!(backward_error(2, &entries, &[0.0, 0.0], &[0.0, 0.0]), 0.0);
        assert!(backward_error(2, &entries, &[1.0, 0.0], &[0.0, 0.0]) > MAX_BACKWARD_ERROR);
        assert_eq!(relative_residual(2, &entries, &[0.0, 0.0], &[0.0, 0.0]), 0.0);
        assert_eq!(relative_residual(2, &entries, &[1.0, 0.0], &[0.0, 0.0]), f64::INFINITY);
        let x = SparseFactor::new(2, &entries).unwrap().solve(&b).unwrap();
        assert!((x[0] - 1.0).abs() < 1e-14 && (x[1] - 1.0).abs() < 1e-14);
        // Singular once rounded: the factorization or the residual guard must
        // refuse it rather than return a silent answer.
        let singular = [(0, 0, 1.0), (0, 1, 1.0), (1, 0, 1.0), (1, 1, 1.0 + 1e-17)];
        assert!(SparseFactor::new(2, &singular).and_then(|f| f.solve(&[1.0, 2.0])).is_err());
    }

    #[test]
    fn backward_error_rejects_a_wrong_factor_after_refinement() {
        let mut factor = SparseFactor::new(1, &[(0, 0, 1.0)]).unwrap();
        factor.entries[0].2 = 2.0;
        let error = factor.solve(&[1.0]).unwrap_err();
        assert!(error.contains("normwise backward error"), "{error}");
    }
}
