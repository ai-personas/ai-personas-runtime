//! Intersections of an infinite axis-aligned line with an exact STL surface.
use crate::neural::{failure, store, Result};
use clap::{Args, ValueEnum};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
    Z,
}
impl Axis {
    pub fn indices(self) -> [usize; 3] {
        match self {
            Self::X => [0, 1, 2],
            Self::Y => [1, 0, 2],
            Self::Z => [2, 0, 1],
        }
    }
}

#[derive(Clone, Debug, Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Optional delivery or request revision. Checks membership and the original file write.
    #[arg(long)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edition: Option<String>,
    /// Exact STL asset revision or byte object, or the retained ZIP containing it.
    pub reference: String,
    /// Case-sensitive member path in the referenced ZIP; omit for a standalone STL.
    #[arg(long)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    /// Measure along X, Y or Z in both directions. The other coordinates are in X/Y/Z order.
    #[arg(long)]
    pub axis: Axis,
    /// First fixed coordinate (Y along X, X along Y or Z).
    #[arg(long, allow_hyphen_values = true)]
    pub u: f64,
    /// Second fixed coordinate (Z along X or Y, Y along Z).
    #[arg(long, allow_hyphen_values = true)]
    pub v: f64,
}
impl Profile {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.u.is_finite() && self.v.is_finite() {
            Ok(())
        } else {
            Err(failure("Use finite line coordinates"))
        }
    }
}
type Point = [f64; 2];
fn orientation(a: Point, b: Point, p: Point) -> (f64, i8) {
    let left = (b[0] - a[0]) * (p[1] - a[1]);
    let right = (b[1] - a[1]) * (p[0] - a[0]);
    let value = left - right;
    let error = 16. * f64::EPSILON * (left.abs() + right.abs());
    (
        value,
        if value.abs() <= error {
            0
        } else if value > 0. {
            1
        } else {
            -1
        },
    )
}
fn between(p: Point, a: Point, b: Point) -> bool {
    (0..2).all(|i| p[i] >= a[i].min(b[i]) && p[i] <= a[i].max(b[i]))
}

/// A shared edge or vertex gets one identity even when a face is triangulated.
/// Adjacent faces calculate its intersection using the same ordered endpoints.
#[derive(Ord, PartialOrd, Eq, PartialEq)]
enum Feature {
    Vertex(usize),
    Edge(usize, usize),
    Face(usize),
}
struct Hit {
    coordinate: f64,
    sign: i8,
    conflicting: bool,
}

fn analyze(bytes: &[u8], query: &Profile) -> Result<Value> {
    query.validate()?;
    let (mesh, min, max) = super::read_mesh(bytes)?;
    let [along, u, v] = query.axis.indices();
    let point = [query.u, query.v];
    let mut contacts = BTreeMap::<Feature, Hit>::new();
    let mut coplanar = 0;
    for (face_id, face) in mesh.faces.iter().enumerate() {
        let indices = face.vertices;
        let xyz =
            indices.map(|i| std::array::from_fn::<_, 3, _>(|k| f64::from(mesh.vertices[i][k])));
        let p = xyz.map(|t| [t[u], t[v]]);
        if (0..2).any(|k| {
            point[k] < p[0][k].min(p[1][k]).min(p[2][k])
                || point[k] > p[0][k].max(p[1][k]).max(p[2][k])
        }) {
            continue;
        }
        let (area, direction) = orientation(p[0], p[1], p[2]);
        let edges = [(1, 2), (2, 0), (0, 1)];
        let det = edges.map(|(a, b)| orientation(p[a], p[b], point));
        if direction == 0 {
            if edges
                .iter()
                .zip(det)
                .any(|(&(a, b), (_, sign))| sign == 0 && between(point, p[a], p[b]))
            {
                coplanar += 1;
            }
            continue;
        }
        if det.iter().any(|&(_, sign)| sign != 0 && sign != direction) {
            continue;
        }
        let zero = det.map(|(_, s)| s == 0);
        let (key, t) = match zero.iter().filter(|&&z| z).count() {
            0 => (
                Feature::Face(face_id),
                xyz[0][along]
                    + det[1].0 / area * (xyz[1][along] - xyz[0][along])
                    + det[2].0 / area * (xyz[2][along] - xyz[0][along]),
            ),
            1 => {
                let (mut a, mut b) = edges[zero.iter().position(|&z| z).unwrap()];
                if indices[a] > indices[b] {
                    std::mem::swap(&mut a, &mut b);
                }
                let k = usize::from((p[b][1] - p[a][1]).abs() > (p[b][0] - p[a][0]).abs());
                let ratio = (point[k] - p[a][k]) / (p[b][k] - p[a][k]);
                (
                    Feature::Edge(indices[a], indices[b]),
                    xyz[a][along] + ratio * (xyz[b][along] - xyz[a][along]),
                )
            }
            2 => {
                let a = zero.iter().position(|&z| !z).unwrap();
                (Feature::Vertex(indices[a]), xyz[a][along])
            }
            _ => {
                coplanar += 1;
                continue;
            }
        };
        let hit = contacts.entry(key).or_insert(Hit {
            coordinate: t,
            sign: direction,
            conflicting: false,
        });
        hit.conflicting |= direction != hit.sign;
    }
    let edge_check = mesh.validate();
    let mut hits: Vec<_> = contacts.into_values().collect();
    hits.sort_by(|a, b| a.coordinate.total_cmp(&b.coordinate));
    let tangent = hits.iter().filter(|h| h.conflicting).count();
    let mut ambiguous = false;
    for pair in hits.windows(2) {
        let a = &pair[0];
        let b = &pair[1];
        let scale = a.coordinate.abs().max(b.coordinate.abs());
        ambiguous |=
            a.sign == b.sign || (b.coordinate - a.coordinate).abs() <= 64. * f64::EPSILON * scale;
    }
    let classified =
        edge_check.is_ok() && coplanar == 0 && tangent == 0 && !ambiguous && hits.len() % 2 == 0;
    let intervals: Vec<_> = hits
        .windows(2)
        .enumerate()
        .map(|(i, pair)| {
            let from = pair[0].coordinate;
            let to = pair[1].coordinate;
            let region = match (classified, i % 2) {
                (false, _) => "unclassified",
                (true, 0) => "inside",
                _ => "outside",
            };
            json!({"from":from, "to":to, "length":to-from, "region":region})
        })
        .collect();
    let names = ["x", "y", "z"];
    Ok(
        json!({"axis":query.axis,"fixed_axes":[names[u],names[v]],"fixed":[query.u,query.v],"axis_bounds":[min[along],max[along]],"triangles":mesh.faces.len(),
        "crossings":hits.iter().map(|h|h.coordinate).collect::<Vec<_>>(),"intervals":intervals,"classified":classified,
        "coplanar_faces":coplanar,"mixed_direction_contacts":tangent,"ambiguous_crossing_order":ambiguous,
        "mesh_validation":{"passed":edge_check.is_ok(),"diagnostic":edge_check.err().map(|e|e.to_string())},
        "method":"Classified spans alternate by even-odd parity between consecutive surface crossings. Coplanar, tangent, ambiguous or invalid mesh sections are unclassified.",
        "units":"STL coordinates have no encoded unit"}),
    )
}

pub async fn run(
    root: &Path,
    query: Profile,
    actor: &str,
    work: Option<&str>,
    invocation: Option<&str>,
) -> Result<Value> {
    let root = root.to_owned();
    let actor = actor.to_owned();
    let work = work.map(str::to_owned);
    let call = invocation.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        let mut store = store::Store::open(&root)?;
        let source = store.snapshot(|db| {
            if let Some(work) = &work {
                store::required(db, "work", work)?;
            }
            super::member::MeshInput::read(db, &query.reference, query.member.as_deref(), query.edition.as_deref())
        })?;
        let profile = analyze(&source.bytes, &query)?;
        let mut result = json!({
            "format":"stl-line-profile-v1", "actor":actor,
            "author_kind":if call.is_some() { "persona" } else { "operator" },
            "work":work, "invocation":call, "source":query.reference,
            "mesh":format!("object:{}", source.digest), "profile":profile
        });
        if let Some(edition) = query.edition { result["package_edition"] = json!(edition); }
        let digest = store.transaction(|db| {
            source.retain(db, &mut result)?;
            store::put(db, &result)
        })?;
        result["receipt"] = json!(format!("object:{digest}"));
        Ok(result)
    })
    .await?
}
