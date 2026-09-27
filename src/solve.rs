use crate::cplx::Cplx;
use crate::linalg::solve_sparse;
use crate::network::SparseY;

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

/// Driving-point impedance at `bus`, or None when that bus is not in `map`.
pub fn zth(y: &SparseY, map: &[Option<usize>], bus: usize) -> Result<Option<Cplx>, String> {
    let Some(k) = map[bus] else {
        return Ok(None);
    };
    if y.n == 0 {
        return Ok(None);
    }
    let mut inj = vec![Cplx::ZERO; y.n];
    inj[k] = Cplx::real(1.0);
    let v = solve_y(y, &inj)?;
    Ok(Some(v[k]))
}
