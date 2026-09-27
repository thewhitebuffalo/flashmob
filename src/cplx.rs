use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub};

/// Rectangular complex number used by the network solver.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cplx {
    pub re: f64,
    pub im: f64,
}

impl Cplx {
    pub const ZERO: Self = Self { re: 0.0, im: 0.0 };

    pub fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    pub fn real(re: f64) -> Self {
        Self { re, im: 0.0 }
    }

    pub fn from_polar(mag: f64, angle_rad: f64) -> Self {
        Self {
            re: mag * angle_rad.cos(),
            im: mag * angle_rad.sin(),
        }
    }

    /// Impedance of a given magnitude and X/R ratio.
    pub fn from_mag_xr(mag: f64, xr: f64) -> Self {
        let den = (1.0 + xr * xr).sqrt();
        let r = mag / den;
        Self { re: r, im: r * xr }
    }

    pub fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }

    pub fn conj(self) -> Self {
        Self {
            re: self.re,
            im: -self.im,
        }
    }

    pub fn inv(self) -> Option<Self> {
        let d = self.re * self.re + self.im * self.im;
        if !d.is_finite() || d == 0.0 {
            None
        } else {
            Some(Self {
                re: self.re / d,
                im: -self.im / d,
            })
        }
    }
}

impl Add for Cplx {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            re: self.re + rhs.re,
            im: self.im + rhs.im,
        }
    }
}

impl AddAssign for Cplx {
    fn add_assign(&mut self, rhs: Self) {
        self.re += rhs.re;
        self.im += rhs.im;
    }
}

impl Sub for Cplx {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self {
            re: self.re - rhs.re,
            im: self.im - rhs.im,
        }
    }
}

impl Neg for Cplx {
    type Output = Self;
    fn neg(self) -> Self {
        Self {
            re: -self.re,
            im: -self.im,
        }
    }
}

impl Mul for Cplx {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self {
            re: self.re * rhs.re - self.im * rhs.im,
            im: self.re * rhs.im + self.im * rhs.re,
        }
    }
}

impl Mul<f64> for Cplx {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self {
            re: self.re * rhs,
            im: self.im * rhs,
        }
    }
}

impl Div for Cplx {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        self * rhs.inv().expect("complex division by zero or non-finite impedance")
    }
}
