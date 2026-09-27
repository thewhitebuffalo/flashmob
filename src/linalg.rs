use faer::linalg::solvers::Solve;
use faer::sparse::{SparseColMat, Triplet};

/// Solve `A x = b` with a sparse LU factorization. Duplicate triplets are summed.
pub fn solve_sparse(n: usize, triplets: &[(usize, usize, f64)], b: &[f64]) -> Result<Vec<f64>, String> {
    if triplets.iter().any(|&(r, c, v)| r >= n || c >= n || !v.is_finite()) || b.iter().any(|v| !v.is_finite()) {
        return Err("matrix or right-hand side contains invalid indices or non-finite values".into());
    }
    if b.len() != n {
        return Err("right-hand side length does not match the matrix".to_string());
    }
    if n == 0 {
        return Ok(Vec::new());
    }
    let trips: Vec<Triplet<usize, usize, f64>> = triplets
        .iter()
        .copied()
        .filter(|(_, _, v)| *v != 0.0)
        .map(|(r, c, v)| Triplet::new(r, c, v))
        .collect();
    if trips.is_empty() {
        return Err("the network matrix is empty".to_string());
    }
    let mat = SparseColMat::<usize, f64>::try_new_from_triplets(n, n, &trips)
        .map_err(|err| format!("could not build the network matrix: {err}"))?;
    let rhs = faer::col::Col::from_fn(n, |i| b[i]);
    let lu = mat
        .sp_lu()
        .map_err(|err| format!("the network matrix is singular: {err}"))?;
    let x = lu.solve(&rhs);
    let mut out = vec![0.0; n];
    for i in 0..n {
        out[i] = x[i];
    }
    if out.iter().any(|v| !v.is_finite()) { return Err("non-finite linear solution".into()); }
    Ok(out)
}
