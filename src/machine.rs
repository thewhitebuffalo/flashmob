//! Duty-specific rotating-machine reactance, after IEEE C37.010-1979 clause
//! 5.4.1. The closing-and-latching (first-cycle) duty and the interrupting
//! duty each scale a machine's reactance by an entered-class multiplier, and
//! small or single-phase induction machines are left out. The multipliers are
//! applied to the machine's whole impedance, so its X/R is unchanged; separate
//! resistance and reactance networks are not built here.

use crate::model::{GeneratorKind, Motor, MotorKind, Project, Source};

/// Which breaker duty a network is built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duty {
    /// Closing and latching, the first-cycle (momentary) network.
    FirstCycle,
    /// Interrupting at contact parting.
    Interrupting,
}

/// What a duty network does with one machine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Treatment {
    /// Multiply the entered machine impedance by this factor.
    Scale(f64),
    /// Omit the machine from this duty network.
    Neglect,
}

/// Per-duty treatment of one machine and any data gap that set it.
#[derive(Clone, Debug, PartialEq)]
pub struct Classified {
    pub first_cycle: Treatment,
    pub interrupting: Treatment,
    /// Short name of the class applied, for notes.
    pub class: &'static str,
    /// Set when classification data were missing and a fallback was used.
    pub warning: Option<String>,
}

impl Classified {
    pub fn for_duty(&self, duty: Duty) -> Treatment {
        match duty { Duty::FirstCycle => self.first_cycle, Duty::Interrupting => self.interrupting }
    }

    fn both(factor: f64, class: &'static str, warning: Option<String>) -> Self {
        Self { first_cycle: Treatment::Scale(factor), interrupting: Treatment::Scale(factor), class, warning }
    }

    fn pair(first_cycle: f64, interrupting: f64, class: &'static str, warning: Option<String>) -> Self {
        Self { first_cycle: Treatment::Scale(first_cycle), interrupting: Treatment::Scale(interrupting), class, warning }
    }
}

/// Smallest three-phase induction motor kept in either duty network, hp.
pub const INDUCTION_MIN_HP: f64 = 50.0;

/// Classify a motor. Missing data never drops a motor silently: without a
/// machine kind the entered subtransient reactance is used for both duties,
/// and an induction motor whose size class needs a speed that was not entered
/// takes the larger-motor class (the lower reactance). Both are reported.
pub fn motor_class(motor: &Motor) -> Classified {
    if motor.phases == Some(1) {
        return Classified { first_cycle: Treatment::Neglect, interrupting: Treatment::Neglect, class: "single-phase motor, neglected", warning: None };
    }
    let Some(kind) = motor.machine_kind else {
        return Classified::both(1.0, "unclassified motor", Some(format!(
            "motor {}: machine_kind is absent, so its subtransient reactance is used unscaled for both closing-and-latching and interrupting duty (IEEE C37.010 5.4.1 multipliers not applied)",
            motor.id
        )));
    };
    match kind {
        MotorKind::Synchronous => Classified::pair(1.0, 1.5, "synchronous motor", None),
        MotorKind::Induction => {
            if motor.hp < INDUCTION_MIN_HP {
                return Classified { first_cycle: Treatment::Neglect, interrupting: Treatment::Neglect, class: "induction motor below 50 hp, neglected", warning: None };
            }
            let large = |bound: f64| motor.hp > bound;
            let (is_large, warning) = if large(1000.0) {
                (true, None)
            } else if !large(250.0) {
                (false, None)
            } else {
                match motor.synchronous_rpm {
                    // At the fixed 60 Hz study frequency the only synchronous
                    // speed above 1800 r/min is 3600 r/min.
                    Some(rpm) if rpm > 1800.0 => (true, None),
                    Some(_) => (false, None),
                    None => (true, Some(format!(
                        "motor {}: synchronous_rpm is absent for a {} hp induction motor, so the larger-motor class (1.0 closing and latching, 1.5 interrupting) was used; enter synchronous_rpm to confirm",
                        motor.id, motor.hp
                    ))),
                }
            };
            if is_large {
                Classified::pair(1.0, 1.5, "large induction motor", warning)
            } else {
                Classified::pair(1.2, 3.0, "medium induction motor", warning)
            }
        }
    }
}

/// Classify a source. A source without a machine kind, a utility, a
/// turbogenerator, a hydro unit with amortisseur windings, and a synchronous
/// condenser keep `mva_sc` for both duties. A hydro unit without amortisseur
/// windings uses 0.75 times its transient reactance, entered as
/// `transient_mva_sc`. Without it the entered `mva_sc` is kept and reported.
pub fn source_class(source: &Source) -> Classified {
    match source.machine_kind {
        None | Some(GeneratorKind::Utility) => Classified::both(1.0, "utility or unclassified source", None),
        Some(GeneratorKind::Turbogenerator) => Classified::both(1.0, "turbogenerator", None),
        Some(GeneratorKind::HydroWithAmortisseur) => Classified::both(1.0, "hydro generator with amortisseur windings", None),
        Some(GeneratorKind::SynchronousCondenser) => Classified::both(1.0, "synchronous condenser", None),
        Some(GeneratorKind::HydroWithoutAmortisseur) => match source.transient_mva_sc {
            // Impedance scales as 1/MVA. 0.75 Xd' over the Xd'' behind mva_sc.
            Some(transient) => Classified::both(0.75 * source.mva_sc / transient, "hydro generator without amortisseur windings", None),
            None => Classified::both(1.0, "hydro generator without amortisseur windings", Some(format!(
                "source {}: transient_mva_sc is absent for a hydro generator without amortisseur windings, so mva_sc is used unscaled for both duties",
                source.id
            ))),
        },
    }
}

/// A copy of `project` with machine impedances set for `duty`. Returns None
/// when every machine is unscaled, so the existing network already applies.
pub fn duty_project(project: &Project, duty: Duty) -> Option<Project> {
    let mut changed = false;
    let mut copy = project.clone();
    copy.motors.retain_mut(|motor| match motor_class(motor).for_duty(duty) {
        Treatment::Neglect => { changed = true; false }
        Treatment::Scale(factor) => {
            if factor != 1.0 { changed = true; motor.x_subtransient_pu *= factor; }
            true
        }
    });
    for source in &mut copy.sources {
        if let Treatment::Scale(factor) = source_class(source).for_duty(duty) {
            if factor != 1.0 { changed = true; source.mva_sc /= factor; }
        }
    }
    changed.then_some(copy)
}

/// Data-gap warnings for every machine.
pub fn warnings(project: &Project) -> Vec<String> {
    let motors = project.motors.iter().filter_map(|m| motor_class(m).warning);
    let sources = project.sources.iter().filter_map(|s| source_class(s).warning);
    motors.chain(sources).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn motor(kind: Option<MotorKind>, hp: f64, rpm: Option<f64>, phases: Option<u8>) -> Motor {
        let mut m = Project::sample().motors[0].clone();
        m.id = "m".into();
        m.machine_kind = kind;
        m.hp = hp;
        m.synchronous_rpm = rpm;
        m.phases = phases;
        m
    }

    fn scales(c: &Classified) -> (f64, f64) {
        match (c.first_cycle, c.interrupting) {
            (Treatment::Scale(a), Treatment::Scale(b)) => (a, b),
            other => panic!("not scaled: {other:?}"),
        }
    }

    fn neglected(c: &Classified) -> bool {
        c.first_cycle == Treatment::Neglect && c.interrupting == Treatment::Neglect
    }

    #[test]
    fn synchronous_motor_row() {
        let c = motor_class(&motor(Some(MotorKind::Synchronous), 500.0, None, None));
        assert_eq!(scales(&c), (1.0, 1.5));
        assert!(c.warning.is_none());
    }

    #[test]
    fn large_induction_rows() {
        for (hp, rpm) in [(1001.0, Some(1800.0)), (1500.0, Some(1200.0)), (251.0, Some(3600.0)), (1001.0, None)] {
            let c = motor_class(&motor(Some(MotorKind::Induction), hp, rpm, Some(3)));
            assert_eq!(scales(&c), (1.0, 1.5), "{hp} hp {rpm:?}");
            assert!(c.warning.is_none());
        }
    }

    #[test]
    fn medium_induction_rows() {
        for (hp, rpm) in [(50.0, Some(1800.0)), (1000.0, Some(1800.0)), (500.0, Some(900.0)), (250.0, Some(3600.0)), (100.0, None)] {
            let c = motor_class(&motor(Some(MotorKind::Induction), hp, rpm, None));
            assert_eq!(scales(&c), (1.2, 3.0), "{hp} hp {rpm:?}");
            assert!(c.warning.is_none());
        }
    }

    #[test]
    fn small_and_single_phase_motors_are_neglected() {
        assert!(neglected(&motor_class(&motor(Some(MotorKind::Induction), 49.9, Some(1800.0), Some(3)))));
        assert!(neglected(&motor_class(&motor(Some(MotorKind::Induction), 500.0, Some(1800.0), Some(1)))));
        assert!(neglected(&motor_class(&motor(None, 500.0, None, Some(1)))));
        // Synchronous machines are never neglected by size.
        assert_eq!(scales(&motor_class(&motor(Some(MotorKind::Synchronous), 10.0, None, Some(3)))), (1.0, 1.5));
    }

    #[test]
    fn missing_kind_keeps_reactance_and_warns() {
        let c = motor_class(&motor(None, 20.0, None, None));
        assert_eq!(scales(&c), (1.0, 1.0));
        assert!(c.warning.unwrap().contains("machine_kind is absent"));
    }

    #[test]
    fn missing_speed_takes_large_class_and_warns() {
        let c = motor_class(&motor(Some(MotorKind::Induction), 600.0, None, None));
        assert_eq!(scales(&c), (1.0, 1.5));
        assert!(c.warning.unwrap().contains("synchronous_rpm is absent"));
    }

    #[test]
    fn generator_rows() {
        let mut s = Project::sample().sources[0].clone();
        for kind in [None, Some(GeneratorKind::Utility), Some(GeneratorKind::Turbogenerator), Some(GeneratorKind::HydroWithAmortisseur), Some(GeneratorKind::SynchronousCondenser)] {
            s.machine_kind = kind;
            assert_eq!(scales(&source_class(&s)), (1.0, 1.0));
        }
        s.machine_kind = Some(GeneratorKind::HydroWithoutAmortisseur);
        s.mva_sc = 60.0;
        s.transient_mva_sc = Some(40.0);
        let c = source_class(&s);
        assert_eq!(scales(&c), (1.125, 1.125));
        let mut p = Project::sample();
        p.sources[0] = s.clone();
        p.motors.clear();
        // 0.75 Xd' behind 40 MVA is 40 / 0.75 MVA.
        let d = duty_project(&p, Duty::Interrupting).unwrap();
        assert!((d.sources[0].mva_sc - 40.0 / 0.75).abs() < 1e-9);
        s.transient_mva_sc = None;
        let c = source_class(&s);
        assert_eq!(scales(&c), (1.0, 1.0));
        assert!(c.warning.unwrap().contains("transient_mva_sc is absent"));
    }

    #[test]
    fn unclassified_project_needs_no_duty_copy() {
        let p = Project::sample();
        assert!(duty_project(&p, Duty::FirstCycle).is_none());
        assert!(duty_project(&p, Duty::Interrupting).is_none());
    }
}
