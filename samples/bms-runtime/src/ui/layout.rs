//! Mount-time, statically typed layout. Resolved leaves supply both paint and hit bounds.
use super::interaction::Bounds;

/// Bitmap text style; components decide which state supplies the text value.
#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    pub scale: usize,
    pub color: u32,
}

#[derive(Clone, Copy)]
pub struct Node<'a, T> {
    pub size: [i64; 2],
    pub content: Content<'a, T>,
}
#[derive(Clone, Copy)]
pub enum Content<'a, T> {
    Leaf(T),
    Row {
        gap: i64,
        children: &'a [Node<'a, T>],
    },
    Column {
        gap: i64,
        children: &'a [Node<'a, T>],
    },
    Layer(&'a [Placed<'a, T>]),
}
#[derive(Clone, Copy)]
pub struct Placed<'a, T> {
    pub origin: [i64; 2],
    pub node: Node<'a, T>,
}
#[derive(Clone, Copy)]
pub struct Resolved<T> {
    pub component: T,
    pub bounds: Bounds,
}
impl<'a, T> Node<'a, T> {
    pub const fn leaf(size: [i64; 2], component: T) -> Self {
        Self {
            size,
            content: Content::Leaf(component),
        }
    }
    pub const fn row(size: [i64; 2], gap: i64, children: &'a [Self]) -> Self {
        Self {
            size,
            content: Content::Row { gap, children },
        }
    }
    pub const fn column(size: [i64; 2], gap: i64, children: &'a [Self]) -> Self {
        Self {
            size,
            content: Content::Column { gap, children },
        }
    }
    pub const fn layer(size: [i64; 2], children: &'a [Placed<'a, T>]) -> Self {
        Self {
            size,
            content: Content::Layer(children),
        }
    }
    pub const fn at(self, x: i64, y: i64) -> Placed<'a, T> {
        Placed {
            origin: [x, y],
            node: self,
        }
    }
}
/// Bounded depth and total nodes protect mount from malformed declarations.
/// No tree is retained or visited by frame updates after this call.
pub fn resolve<T: Copy>(root: Node<'_, T>) -> Result<Vec<Resolved<T>>, String> {
    fn visit<T: Copy>(
        node: Node<'_, T>,
        origin: [i64; 2],
        parent: Bounds,
        depth: usize,
        count: &mut usize,
        out: &mut Vec<Resolved<T>>,
    ) -> Result<(), String> {
        *count += 1;
        if depth > 32 || *count > 1024 {
            return Err("layout exceeds depth/node limit".into());
        }
        if node.size.iter().any(|&n| n <= 0) || origin.iter().any(|&n| n < 0) {
            return Err("layout requires positive extents and nonnegative origins".into());
        }
        let bounds = Bounds {
            x: parent.x.checked_add(origin[0]).ok_or("layout x overflow")?,
            y: parent.y.checked_add(origin[1]).ok_or("layout y overflow")?,
            width: node.size[0],
            height: node.size[1],
        };
        let right = bounds
            .x
            .checked_add(bounds.width)
            .ok_or("layout right overflow")?;
        let bottom = bounds
            .y
            .checked_add(bounds.height)
            .ok_or("layout bottom overflow")?;
        if right
            > parent
                .x
                .checked_add(parent.width)
                .ok_or("layout parent overflow")?
            || bottom
                > parent
                    .y
                    .checked_add(parent.height)
                    .ok_or("layout parent overflow")?
        {
            return Err("layout child exceeds its region".into());
        }
        match node.content {
            Content::Leaf(component) => {
                out.try_reserve(1).map_err(|_| "layout allocation failed")?;
                out.push(Resolved { component, bounds });
            }
            Content::Layer(children) => {
                for child in children {
                    visit(child.node, child.origin, bounds, depth + 1, count, out)?;
                }
            }
            Content::Row { gap, children } | Content::Column { gap, children } => {
                if gap < 0 {
                    return Err("layout gap must be nonnegative".into());
                }
                let axis = usize::from(matches!(node.content, Content::Column { .. }));
                let mut offset = [0i64, 0i64];
                for (index, child) in children.iter().enumerate() {
                    if index != 0 {
                        offset[axis] =
                            offset[axis].checked_add(gap).ok_or("layout gap overflow")?;
                    }
                    visit(*child, offset, bounds, depth + 1, count, out)?;
                    offset[axis] = offset[axis]
                        .checked_add(child.size[axis])
                        .ok_or("layout extent overflow")?;
                }
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    visit(
        root,
        [0, 0],
        Bounds {
            x: 0,
            y: 0,
            width: root.size[0],
            height: root.size[1],
        },
        0,
        &mut 0,
        &mut out,
    )?;
    Ok(out)
}

#[cfg(test)]
#[path = "layout_fixtures.rs"]
mod fixtures;
