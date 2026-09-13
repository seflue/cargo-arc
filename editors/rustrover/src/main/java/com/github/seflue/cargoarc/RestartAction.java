package com.github.seflue.cargoarc;

import com.intellij.openapi.actionSystem.ActionUpdateThread;
import com.intellij.openapi.actionSystem.AnAction;
import com.intellij.openapi.actionSystem.AnActionEvent;
import com.intellij.openapi.project.Project;

/** Restarts the running cargo-arc service. */
public final class RestartAction extends AnAction {

    @Override
    public void actionPerformed(AnActionEvent event) {
        Project project = event.getProject();
        if (project != null) {
            project.getService(ArcService.class).restart();
        }
    }

    @Override
    public void update(AnActionEvent event) {
        Project project = event.getProject();
        boolean running = project != null && project.getService(ArcService.class).isRunning();
        event.getPresentation().setEnabled(running);
    }

    @Override
    public ActionUpdateThread getActionUpdateThread() {
        return ActionUpdateThread.BGT;
    }
}
