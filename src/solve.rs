use crate::cplx::Cplx;
#[cfg(test)]
use crate::linalg::solve_sparse;
use crate::linalg::SparseFactor;
use crate::network::SparseY;

#[cfg(test)]
pub fn solve_y(y: &SparseY, injection: &[Cplx]) -> Result<Vec<Cplx>, String> {
    let n = y.n;
    if injection.len() != n {
        return Err("injection vector does not match the network".into());
    }
    if n == 0 {
        return Ok(Vec::new());
    }
    let mut trips = Vec::new();
    for (i, row) in y.rows().iter().enumerate() {
        for &(j, yij) in row {
            trips.push((2 * i, 2 * j, yij.re));
            trips.push((2 * i, 2 * j + 1, -yij.im));
            trips.push((2 * i + 1, 2 * j, yij.im));
            trips.push((2 * i + 1, 2 * j + 1, yij.re));
        }
    }
    let mut rhs = vec![0.0; n * 2];
    for i in 0..n {
        rhs[2 * i] = injection[i].re;
        rhs[2 * i + 1] = injection[i].im;
    }
    let x = solve_sparse(n * 2, &trips, &rhs)?;
    Ok((0..n).map(|i| Cplx::new(x[2 * i], x[2 * i + 1])).collect())
}

/// Factors a sequence network on its first requested column, then reuses the LU.
/// Lazy construction preserves invalid-prefault and unenergized-island behavior.
pub struct AdmittanceSolver<'a> {
    y: &'a SparseY,
    factor: Option<SparseFactor>,
    rhs: Vec<f64>,
}

impl<'a> AdmittanceSolver<'a> {
    pub fn new(y: &'a SparseY) -> Self {
        Self { y, factor: None, rhs: vec![0.0; 2 * y.n] }
    }

    pub fn column(&mut self, k: usize) -> Result<Vec<Cplx>, String> {
        if k >= self.y.n { return Err("fault bus is outside the reduced network".into()); }
        if self.factor.is_none() {
            let nnz: usize = self.y.rows().iter().map(Vec::len).sum();
            let mut trips = Vec::with_capacity(4 * nnz);
            for (i, row) in self.y.rows().iter().enumerate() {
                for &(j, y) in row {
                    trips.extend_from_slice(&[
                        (2*i, 2*j, y.re), (2*i, 2*j+1, -y.im),
                        (2*i+1, 2*j, y.im), (2*i+1, 2*j+1, y.re),
                    ]);
                }
            }
            self.factor = Some(SparseFactor::new(2 * self.y.n, &trips)?);
        }
        self.rhs[2*k] = 1.0;
        let result = self.factor.as_ref().unwrap().solve(&self.rhs);
        self.rhs[2*k] = 0.0;
        let x = result?;
        Ok((0..self.y.n).map(|i| Cplx::new(x[2*i], x[2*i+1])).collect())
    }

    pub fn zth(&mut self, k: Option<usize>) -> Result<Option<Cplx>, String> {
        k.map(|k| self.column(k).map(|v| v[k])).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reused_columns_satisfy_complex_network_and_scaled_fault_injections() {
        // Asymmetric off-diagonals exercise the real block representation used
        // for phase-shifting transformers, including non-real fault injections.
        let mut y = SparseY::new(3);
        for (i, j, re, im) in [
            (0, 0, 4.0, -8.0), (1, 1, 5.0, -9.0), (2, 2, 3.0, -6.0),
            (0, 1, -1.0, 2.0), (1, 0, -2.0, 1.0),
            (1, 2, -0.5, 1.0), (2, 1, -1.0, 0.5),
        ] { y.add(i, j, Cplx::new(re, im)); }
        let mut solver = AdmittanceSolver::new(&y);
        for k in [2, 0, 1, 2] {
            let column = solver.column(k).unwrap();
            let residual = y.mul(&column);
            for (i, value) in residual.iter().enumerate() {
                let expected = if i == k { Cplx::real(1.0) } else { Cplx::ZERO };
                assert!((*value - expected).abs() < 1e-12);
            }
            let fault_current = Cplx::new(0.9, -0.3) / column[k];
            let scaled: Vec<_> = column.iter().map(|v| *v * fault_current).collect();
            let mut injection = vec![Cplx::ZERO; y.n];
            injection[k] = fault_current;
            let direct = solve_y(&y, &injection).unwrap();
            for (a, b) in scaled.iter().zip(direct) {
                assert!((*a - b).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn unused_empty_or_singular_sequences_are_not_factored() {
        for n in [0, 2] {
            let y = SparseY::new(n);
            let mut solver = AdmittanceSolver::new(&y);
            assert!(solver.zth(None).unwrap().is_none());
            assert!(solver.column(0).is_err());
        }
    }
}
