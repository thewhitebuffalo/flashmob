use faer::linalg::solvers::Solve;
use faer::sparse::linalg::solvers::Lu;
use faer::sparse::{SparseColMat, Triplet};

/// Owned factorization, reusable for any right-hand side of the same matrix.
pub struct SparseFactor {
    n: usize,
    lu: Option<Lu<usize, f64>>,
}

impl SparseFactor {
    pub fn new(n: usize, triplets: &[(usize, usize, f64)]) -> Result<Self, String> {
        if triplets.iter().any(|&(r, c, v)| r >= n || c >= n || !v.is_finite()) {
            return Err("matrix or right-hand side contains invalid indices or non-finite values".into());
        }
        if n == 0 { return Ok(Self { n, lu: None }); }
        let trips: Vec<Triplet<usize, usize, f64>> = triplets.iter().copied()
            .filter(|(_, _, v)| *v != 0.0)
            .map(|(r, c, v)| Triplet::new(r, c, v)).collect();
        if trips.is_empty() { return Err("the network matrix is empty".into()); }
        let mat = SparseColMat::<usize, f64>::try_new_from_triplets(n, n, &trips)
            .map_err(|err| format!("could not build the network matrix: {err}"))?;
        let lu = mat.sp_lu()
            .map_err(|err| format!("the network matrix is singular: {err}"))?;
        Ok(Self { n, lu: Some(lu) })
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
        let out: Vec<f64> = (0..self.n).map(|i| x[i]).collect();
        if out.iter().any(|v| !v.is_finite()) { return Err("non-finite linear solution".into()); }
        Ok(out)
    }
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
}
