//! Shallow UIA tree text dump for **Diagnose** (M9).

use std::collections::VecDeque;

use windows::Win32::UI::Accessibility::{
    IUIAutomation, IUIAutomationElement, TreeScope_Children, UIA_CONTROLTYPE_ID,
};
use windows::core::BSTR;

use crate::UiaError;
use crate::hwnd::UiaHwnd;

/// Text snapshot of the UIA tree from `hwnd` (breadth-first, bounded depth and node count).
pub fn snapshot_uia_tree(
    automation: &IUIAutomation,
    hwnd: UiaHwnd,
    max_depth: usize,
    max_nodes: usize,
) -> Result<String, UiaError> {
    if hwnd.is_invalid() {
        return Ok("(null hwnd)\n".to_string());
    }

    let root = unsafe { automation.ElementFromHandle(hwnd) }
        .map_err(|e| UiaError::Operation(format!("ElementFromHandle: {e}")))?;

    let true_cond = unsafe { automation.CreateTrueCondition() }
        .map_err(|e| UiaError::Operation(format!("CreateTrueCondition: {e}")))?;

    let mut out = String::new();
    out.push_str(&format!(
        "UIA diagnose (max_depth={max_depth}, max_nodes={max_nodes})\nhwnd={hwnd:?}\n---\n"
    ));

    let name_root = element_name_brief(&root);
    out.push_str(&format!("0: {name_root}\n"));

    #[derive(Clone)]
    struct Frame {
        el: IUIAutomationElement,
        depth: usize,
        path: String,
    }

    let mut q: VecDeque<Frame> = VecDeque::new();
    q.push_back(Frame {
        el: root,
        depth: 0,
        path: "0".to_string(),
    });

    let mut emitted = 1usize;
    while let Some(frame) = q.pop_front() {
        if frame.depth >= max_depth {
            continue;
        }
        let children = match unsafe { frame.el.FindAll(TreeScope_Children, &true_cond) } {
            Ok(a) => a,
            Err(_) => continue,
        };
        let len = unsafe { children.Length() }.unwrap_or(0);
        for i in 0i32..len {
            if emitted >= max_nodes {
                out.push_str("... (truncated)\n");
                return Ok(out);
            }
            let el = match unsafe { children.GetElement(i) } {
                Ok(e) => e,
                Err(_) => continue,
            };
            let child_path = format!("{}.{i}", frame.path);
            let line = format!("{child_path}: {}\n", element_name_brief(&el));
            out.push_str(&line);
            emitted += 1;
            q.push_back(Frame {
                el,
                depth: frame.depth + 1,
                path: child_path,
            });
        }
    }

    Ok(out)
}

fn element_name_brief(el: &IUIAutomationElement) -> String {
    let ct = unsafe { el.CurrentControlType() }.unwrap_or(UIA_CONTROLTYPE_ID(0));
    let name_b: BSTR = unsafe { el.CurrentName() }.unwrap_or_default();
    let name = name_b.to_string();
    let name_disp = if name.is_empty() {
        "<empty>"
    } else {
        name.as_str()
    };
    format!("control_type={} name={name_disp}", ct.0)
}
