use std::process::Command;

use anyhow::{Context as _, Result};
use assert_cmd::prelude::*;

fn run_inst_verbose_exec(src: &str) -> Result<String> {
    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["exec", src, "-d", "inst-v", "-p"])
        .output()
        .context("run wq exec")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    Ok(format!("{stdout}{stderr}"))
}

#[test]
fn streamed_exec_constprop_uses_prior_global_values() -> Result<()> {
    let output = run_inst_verbose_exec("a:1\nb:a+2\nb")?;

    assert!(
        output.contains(
            "Inst @ constprop\n   0 LoadConst(Int(3))\n   1 StoreVarKeep(\"b\")\n   2 Return"
        ),
        "streamed assignment should fold using a from the previous chunk:\n{output}",
    );
    Ok(())
}

#[test]
fn streamed_exec_constprop_seeds_global_closure_captures() -> Result<()> {
    let output = run_inst_verbose_exec("a:1\nf:{a+2}\nf")?;

    assert!(
        output.contains(
            "Inst @ constprop\n   0 LoadClosure: params=None locals=3 captures=[Global(a)] names=[x, y, z]\n     {\n   0   LoadConst(Int(3))\n   1   Return\n     }\n   1 StoreVarKeep(\"f\")\n   2 Return"
        ),
        "streamed closure should fold captured global a from the previous chunk:\n{output}",
    );
    Ok(())
}
