use super::*;

fn s() -> Settings {
    Settings::default()
}

/// A generator of numbers with a fixed seed, the same on every system.
struct Seed(u64);

impl Seed {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// A whole number from `-range` to `range`.
    fn pick(&mut self, range: i64) -> i64 {
        (self.next() % (2 * range as u64 + 1)) as i64 - range
    }
}

#[test]
fn the_shipped_settings_are_the_defaults_and_a_user_file_changes_only_what_it_names() {
    let shipped = Settings::from_cfg_text("").unwrap();
    assert_eq!(shipped, Settings::default());
    assert_eq!(shipped.max_iterations, 100);
    assert!((shipped.tolerance - 1e-8).abs() < 1e-20);
    let user = Settings::from_cfg_text("[solver]\nmax_iterations = 7  # few\ntolerance = 0.001\n")
        .unwrap();
    assert_eq!(user.max_iterations, 7);
    assert!((user.tolerance - 0.001).abs() < 1e-18);
    assert_eq!(user.fd_step, shipped.fd_step);
}

#[test]
fn a_settings_text_that_is_not_understood_is_refused_with_its_line() {
    for (text, part) in [
        ("[solver]\nnot_a_key = 1\n", "unknown key"),
        ("[solver]\ntolerance = abc\n", "not a number"),
        ("[solver]\ntolerance = 5\n", "outside"),
        ("[solver]\nmax_iterations = 0\n", "outside"),
        ("[other]\ntolerance = 1e-6\n", "not [solver]"),
        ("tolerance = 1e-6\n", "outside [solver]"),
        ("[solver]\ntolerance\n", "not key = value"),
        ("[solver]\nmax_iterations = -3\n", "whole number"),
        ("[solver]\ndamping_factor = 1\n", "outside"),
    ] {
        let e = Settings::from_cfg_text(text).unwrap_err();
        assert_eq!(e.code(), "SKSOLVER-001", "{text}");
        assert!(e.to_string().contains(part), "{text}: {e}");
    }
}

#[test]
fn fixing_every_variable_leaves_nothing_free() {
    let mut sys = System::new();
    let x = sys.variable(0.0);
    let y = sys.variable(0.0);
    sys.fix(x, 3.0).unwrap();
    sys.fix(y, -4.5).unwrap();
    let v = sys.solve(&s()).unwrap();
    assert!(matches!(v, Verdict::Solved { dof: 0, .. }), "{v:?}");
    assert!((sys.value(x).unwrap() - 3.0).abs() < 1e-7);
    assert!((sys.value(y).unwrap() + 4.5).abs() < 1e-7);
}

#[test]
fn the_degrees_of_freedom_are_the_unknowns_minus_the_rank() {
    let mut sys = System::new();
    let v: Vec<VarId> = (0..3).map(|i| sys.variable(f64::from(i))).collect();
    assert_eq!(sys.degrees_of_freedom(&s()).unwrap(), 3);
    sys.equal(v[0], v[1]).unwrap();
    let verdict = sys.solve(&s()).unwrap();
    assert!(
        matches!(verdict, Verdict::Solved { dof: 2, .. }),
        "{verdict:?}"
    );
    assert_eq!(sys.degrees_of_freedom(&s()).unwrap(), 2);
    sys.fix(v[2], 9.0).unwrap();
    assert!(matches!(
        sys.solve(&s()).unwrap(),
        Verdict::Solved { dof: 1, .. }
    ));
}

#[test]
fn a_distance_between_two_points_leaves_the_turn_free() {
    let mut sys = System::new();
    let (ax, ay) = (sys.variable(0.0), sys.variable(0.0));
    let (bx, by) = (sys.variable(3.0), sys.variable(1.0));
    sys.fix(ax, 0.0).unwrap();
    sys.fix(ay, 0.0).unwrap();
    sys.distance((ax, ay), (bx, by), 5.0).unwrap();
    let verdict = sys.solve(&s()).unwrap();
    assert!(
        matches!(verdict, Verdict::Solved { dof: 1, .. }),
        "{verdict:?}"
    );
    let (x, y) = (sys.value(bx).unwrap(), sys.value(by).unwrap());
    assert!((x * x + y * y - 25.0).abs() < 1e-6);
}

#[test]
fn a_constraint_that_repeats_another_is_redundant_and_nothing_moves() {
    let mut sys = System::new();
    let x = sys.variable(10.0);
    let first = sys.fix(x, 2.0).unwrap();
    let second = sys.fix(x, 2.0).unwrap();
    let v = sys.solve(&s()).unwrap();
    match v {
        Verdict::OverConstrained {
            redundant,
            conflicting,
        } => {
            assert!(conflicting.is_empty());
            assert!(redundant.contains(&second.0), "{redundant:?}");
            assert!(redundant.contains(&first.0), "{redundant:?}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(sys.value(x).unwrap(), 10.0);
}

#[test]
fn two_values_for_one_variable_are_a_conflict_and_nothing_moves() {
    let mut sys = System::new();
    let x = sys.variable(0.0);
    let y = sys.variable(0.0);
    sys.fix(y, 1.0).unwrap();
    sys.fix(x, 1.0).unwrap();
    let second = sys.fix(x, 2.0).unwrap();
    let v = sys.solve(&s()).unwrap();
    match v {
        Verdict::OverConstrained {
            redundant,
            conflicting,
        } => {
            assert!(redundant.is_empty(), "{redundant:?}");
            assert!(conflicting.contains(&second.0), "{conflicting:?}");
            // the constraint on y has nothing to do with it
            assert!(!conflicting.contains(&0), "{conflicting:?}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(sys.value(x).unwrap(), 0.0);
    assert_eq!(sys.value(y).unwrap(), 0.0);
}

#[test]
fn a_disabled_constraint_is_not_in_the_problem_and_can_come_back() {
    let mut sys = System::new();
    let x = sys.variable(0.0);
    sys.fix(x, 1.0).unwrap();
    let other = sys.fix(x, 2.0).unwrap();
    assert!(matches!(
        sys.solve(&s()).unwrap(),
        Verdict::OverConstrained { .. }
    ));
    sys.set_enabled(other, false).unwrap();
    assert!(matches!(
        sys.solve(&s()).unwrap(),
        Verdict::Solved { dof: 0, .. }
    ));
    assert!((sys.value(x).unwrap() - 1.0).abs() < 1e-7);
    sys.set_enabled(other, true).unwrap();
    assert!(matches!(
        sys.solve(&s()).unwrap(),
        Verdict::OverConstrained { .. }
    ));
    assert_eq!(sys.name(other), Some("fix"));
    assert!(sys.set_enabled(ConstraintId(99), true).is_err());
}

#[test]
fn running_out_of_steps_is_not_converged_and_nothing_moves() {
    let mut sys = System::new();
    let x = sys.variable(100.0);
    sys.constraint("cube", 1, move |v| vec![v[x.0] * v[x.0] * v[x.0] - 1.0])
        .unwrap();
    let few = Settings::from_cfg_text("[solver]\nmax_iterations = 2\n").unwrap();
    let v = sys.solve(&few).unwrap();
    assert!(
        matches!(v, Verdict::NotConverged { iterations: 2, .. }),
        "{v:?}"
    );
    assert_eq!(sys.value(x).unwrap(), 100.0);
    // with the shipped steps it gets there
    assert!(matches!(
        sys.solve(&s()).unwrap(),
        Verdict::Solved { dof: 0, .. }
    ));
    assert!((sys.value(x).unwrap() - 1.0).abs() < 1e-6);
}

/// The two crossings of a circle and a line, as a problem of its own.
struct CircleAndLine {
    owners: Vec<Option<usize>>,
}

impl Problem for CircleAndLine {
    fn unknowns(&self) -> usize {
        2
    }

    fn residuals(&self, x: &[f64]) -> Vec<f64> {
        vec![(x[0] * x[0] + x[1] * x[1]).sqrt() - 5.0, x[1] - 3.0]
    }

    fn owners(&self) -> &[Option<usize>] {
        &self.owners
    }
}

#[test]
fn a_problem_of_the_callers_own_is_solved_on_the_side_it_starts() {
    let p = CircleAndLine {
        owners: vec![Some(0), Some(1)],
    };
    for (start, want) in [([3.0, 1.0], 4.0), ([-3.0, 1.0], -4.0)] {
        let r = solve(&p, &start, &s()).unwrap();
        assert!(
            matches!(r.verdict, Verdict::Solved { dof: 0, .. }),
            "{:?}",
            r.verdict
        );
        assert!((r.x[0] - want).abs() < 1e-6, "{:?}", r.x);
        assert!((r.x[1] - 3.0).abs() < 1e-6);
    }
}

#[test]
fn a_problem_that_is_too_large_or_badly_made_is_refused() {
    let p = CircleAndLine {
        owners: vec![Some(0), Some(1)],
    };
    // the values do not match the unknowns
    assert_eq!(solve(&p, &[1.0], &s()).unwrap_err().code(), "SKSOLVER-003");
    let small = Settings::from_cfg_text("[solver]\nmax_unknowns = 1\n").unwrap();
    assert_eq!(
        solve(&p, &[1.0, 1.0], &small).unwrap_err().code(),
        "SKSOLVER-002"
    );
    let few_rows = Settings::from_cfg_text("[solver]\nmax_rows = 1\n").unwrap();
    assert_eq!(
        degrees_of_freedom(&p, &[1.0, 1.0], &few_rows)
            .unwrap_err()
            .code(),
        "SKSOLVER-002"
    );
    // the rows do not match the owners
    let bad = CircleAndLine {
        owners: vec![Some(0)],
    };
    assert_eq!(
        solve(&bad, &[1.0, 1.0], &s()).unwrap_err().code(),
        "SKSOLVER-004"
    );
    // a residual that is not a number at the start
    let mut sys = System::new();
    let v = sys.variable(0.0);
    sys.constraint("nan", 1, move |x| vec![x[v.0] / 0.0 - x[v.0] / 0.0])
        .unwrap();
    assert_eq!(sys.solve(&s()).unwrap_err().code(), "SKSOLVER-004");
    assert!(sys.constraint("none", 0, |_| Vec::new()).is_err());
    assert!(sys.value(VarId(5)).is_err());
    assert!(sys.fix(VarId(5), 1.0).is_err());
}

#[test]
fn the_same_system_is_solved_to_the_same_bits_every_time() {
    let build = || {
        let mut sys = System::new();
        let p: Vec<(VarId, VarId)> = (0..4)
            .map(|i| {
                (
                    sys.variable(f64::from(i) * 1.3),
                    sys.variable(f64::from(i * i) * 0.7),
                )
            })
            .collect();
        sys.fix(p[0].0, 0.0).unwrap();
        sys.fix(p[0].1, 0.0).unwrap();
        for w in p.windows(2) {
            sys.distance(w[0], w[1], 4.0).unwrap();
        }
        sys.equal(p[1].1, p[0].1).unwrap();
        (sys, p)
    };
    let (mut a, pa) = build();
    let (mut b, pb) = build();
    let (va, vb) = (a.solve(&s()).unwrap(), b.solve(&s()).unwrap());
    assert_eq!(va, vb);
    for (x, y) in pa.iter().zip(&pb) {
        assert_eq!(
            a.value(x.0).unwrap().to_bits(),
            b.value(y.0).unwrap().to_bits()
        );
        assert_eq!(
            a.value(x.1).unwrap().to_bits(),
            b.value(y.1).unwrap().to_bits()
        );
    }
}

#[test]
fn random_targets_are_reached_whatever_the_order_of_the_constraints() {
    let mut seed = Seed(0x5eed_0001);
    for round in 0..40 {
        let n = 2 + (seed.next() % 6) as usize;
        let starts: Vec<f64> = (0..n).map(|_| seed.pick(50) as f64).collect();
        let targets: Vec<f64> = (0..n).map(|_| seed.pick(200) as f64 / 4.0).collect();
        let mut order: Vec<usize> = (0..n).collect();
        // a shuffle that depends on the seed only
        for i in (1..n).rev() {
            order.swap(i, (seed.next() % (i as u64 + 1)) as usize);
        }
        let solve_in = |order: &[usize]| {
            let mut sys = System::new();
            let vars: Vec<VarId> = starts.iter().map(|v| sys.variable(*v)).collect();
            for &i in order {
                sys.fix(vars[i], targets[i]).unwrap();
            }
            let v = sys.solve(&s()).unwrap();
            assert!(
                matches!(v, Verdict::Solved { dof: 0, .. }),
                "round {round}: {v:?}"
            );
            vars.iter()
                .map(|v| sys.value(*v).unwrap())
                .collect::<Vec<f64>>()
        };
        let straight: Vec<usize> = (0..n).collect();
        let (a, b) = (solve_in(&straight), solve_in(&order));
        for i in 0..n {
            assert!((a[i] - targets[i]).abs() < 1e-6, "round {round}");
            assert!((b[i] - targets[i]).abs() < 1e-6, "round {round}");
        }
    }
}

#[test]
fn random_linear_systems_with_a_dominant_diagonal_have_one_solution() {
    let mut seed = Seed(0x5eed_0002);
    for round in 0..30 {
        let n = 2 + (seed.next() % 5) as usize;
        let mut m = vec![vec![0.0f64; n]; n];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, e) in row.iter_mut().enumerate() {
                *e = if i == j {
                    10.0 + seed.pick(3) as f64
                } else {
                    seed.pick(2) as f64
                };
            }
        }
        let want: Vec<f64> = (0..n).map(|_| seed.pick(20) as f64).collect();
        let rhs: Vec<f64> = m
            .iter()
            .map(|row| row.iter().zip(&want).map(|(a, b)| a * b).sum())
            .collect();
        let mut sys = System::new();
        let vars: Vec<VarId> = (0..n).map(|_| sys.variable(0.0)).collect();
        for i in 0..n {
            let (row, b) = (m[i].clone(), rhs[i]);
            sys.constraint("row", 1, move |x| {
                vec![row.iter().zip(x).map(|(a, v)| a * v).sum::<f64>() - b]
            })
            .unwrap();
        }
        let v = sys.solve(&s()).unwrap();
        assert!(
            matches!(v, Verdict::Solved { dof: 0, .. }),
            "round {round}: {v:?}"
        );
        for (var, w) in vars.iter().zip(&want) {
            assert!((sys.value(*var).unwrap() - w).abs() < 1e-5, "round {round}");
        }
    }
}

#[test]
fn the_rank_of_random_equations_is_found_whatever_the_number_of_repeats() {
    // k independent equations and r copies of the first: dof = n - k, and exactly the copies
    // are named as redundant
    let mut seed = Seed(0x5eed_0003);
    for round in 0..25 {
        let n = 3 + (seed.next() % 4) as usize;
        let k = 1 + (seed.next() % (n as u64 - 1)) as usize;
        let copies = 1 + (seed.next() % 3) as usize;
        let mut sys = System::new();
        let vars: Vec<VarId> = (0..n).map(|_| sys.variable(seed.pick(5) as f64)).collect();
        let mut ids = Vec::new();
        for (i, v) in vars.iter().take(k).enumerate() {
            ids.push(sys.fix(*v, i as f64 + 1.0).unwrap());
        }
        for _ in 0..copies {
            ids.push(sys.fix(vars[0], 1.0).unwrap());
        }
        match sys.solve(&s()).unwrap() {
            Verdict::OverConstrained {
                redundant,
                conflicting,
            } => {
                assert!(conflicting.is_empty(), "round {round}");
                for c in &ids[k..] {
                    assert!(redundant.contains(&c.0), "round {round}: {redundant:?}");
                }
            }
            other => panic!("round {round}: {other:?}"),
        }
        // without the copies the rank is k
        for c in &ids[k..] {
            sys.set_enabled(*c, false).unwrap();
        }
        let v = sys.solve(&s()).unwrap();
        assert!(
            matches!(v, Verdict::Solved { dof, .. } if dof == n - k),
            "round {round}: {v:?}"
        );
    }
}

#[test]
fn the_norm_never_falls_to_zero() {
    assert_eq!(norm2(3.0, 4.0), 5.0);
    assert!(norm2(0.0, 0.0) > 0.0);
}

#[test]
fn the_codes_and_texts_of_the_reasons_are_stable() {
    let all = [
        SolverError::Settings("a".into()),
        SolverError::TooLarge("b".into()),
        SolverError::Unknown("c".into()),
        SolverError::Residuals("d".into()),
    ];
    let codes: Vec<&str> = all.iter().map(SolverError::code).collect();
    assert_eq!(
        codes,
        [
            "SKSOLVER-001",
            "SKSOLVER-002",
            "SKSOLVER-003",
            "SKSOLVER-004"
        ]
    );
    for e in &all {
        assert!(e.to_string().starts_with(e.code()));
    }
}

#[test]
fn damaged_settings_never_panic_and_what_is_read_is_in_range_with_a_fixed_seed() {
    let mut seed = Seed(0x5eed_0004);
    let (mut read, mut refused) = (0u32, 0u32);
    for round in 0..4000 {
        let mut bytes = DEFAULT_SETTINGS.as_bytes().to_vec();
        for _ in 0..1 + seed.pick(2).unsigned_abs() {
            let at = (seed.next() % bytes.len() as u64) as usize;
            match seed.next() % 4 {
                0 => bytes[at] ^= 1 << (seed.next() % 7),
                1 => bytes.truncate(at),
                2 => {
                    bytes.remove(at);
                }
                _ => bytes.insert(at, b"0123456789.e-= #[]"[(seed.next() % 18) as usize]),
            }
            if bytes.is_empty() {
                break;
            }
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        match Settings::from_cfg_text(&text) {
            Ok(s) => {
                read += 1;
                assert!(s.tolerance > 0.0 && s.tolerance <= 1.0, "round {round}");
                assert!(
                    s.max_iterations >= 1 && s.damping_factor > 1.0,
                    "round {round}"
                );
                assert!(s.max_unknowns >= 1 && s.max_rows >= 1, "round {round}");
            }
            Err(e) => {
                refused += 1;
                assert_eq!(e.code(), "SKSOLVER-001", "round {round}: {e}");
            }
        }
    }
    assert!(
        read > 100 && refused > 100,
        "{read} read, {refused} refused"
    );
}
