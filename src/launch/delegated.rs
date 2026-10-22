//! Continue an actual delegation subtree under one foreground execution owner.
//! Concurrent native drivers share one coordinator across the subtree.
use super::*;

pub(super) fn subtree(state: &State, root: &str) -> Result<Vec<String>> {
    let mut works = vec![root.to_owned()];
    let mut selected = BTreeSet::from([root.to_owned()]);
    let Some(episode) = need_proposals::episode(state, root) else {
        return Ok(works);
    };
    need_proposals::validate_state(state)?;
    let mut index = 0;
    while index < works.len() {
        let parent = works[index].clone();
        for child in episode.children() {
            if child.parent_work(episode) == parent && selected.insert(child.binding.work.clone()) {
                works.push(child.binding.work.clone());
            }
        }
        index += 1;
    }
    Ok(works)
}

pub(super) async fn run<D: Driver>(
    app: &mut Runtime,
    root: &str,
    driver: &mut D,
    owner: &s::ExecutionOwner,
) -> Result<Output> {
    if driver.concurrent() {
        return parallel::run_delegated(app, root, driver, owner).await;
    }
    let mut failure = None;
    loop {
        driver.before_step(app)?;
        app.publish_completed_host_commands(unix_now()?)?;
        let works = subtree(&app.snapshot()?, root)?;
        let mut progress = false;
        for work in works {
            driver.before_step(app)?;
            let admitted = intake_delegation::admit_pending_for_work_owned(app, &work, owner)?;
            progress |= !admitted.is_empty();
            let steps_before = load(app)?.1.steps;
            let output = run_with_options(app, &work, driver, owner, true).await?;
            progress |= persona_birth::create_pending_for_work_owned(app, &work, driver, owner)?;
            let admitted = intake_delegation::admit_pending_for_work_owned(app, &work, owner)?;
            progress |= !admitted.is_empty() || load(app)?.1.steps > steps_before;
            if let Some(reason) = output.stop {
                if !matches!(reason.as_str(), "work_step_yielded" | "no_eligible_route" | "held") {
                    failure.get_or_insert(reason);
                }
            }
        }
        // Recompute the subtree after every round so descendants requested by
        // newly admitted children join without recursion or a depth ceiling.
        if progress {
            continue;
        }
        if app.has_background_host_commands() {
            app.wait_for_host_command().await?;
            continue;
        }
        let mut output = result(app, root)?;
        // A completed child never resumes an author's explicit Stop or Pause.
        output.stop = Some(failure.unwrap_or_else(|| "no_eligible_route".to_owned()));
        if output.stop.as_deref() == Some("no_eligible_route")
            && !active_invocations(&app.snapshot()?).is_empty()
        {
            output.stop = Some("held".to_owned());
        }
        return Ok(output);
    }
}
