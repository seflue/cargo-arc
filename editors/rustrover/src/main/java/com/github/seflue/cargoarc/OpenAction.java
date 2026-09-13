package com.github.seflue.cargoarc;

import com.intellij.openapi.actionSystem.ActionUpdateThread;
import com.intellij.openapi.actionSystem.AnAction;
import com.intellij.openapi.actionSystem.AnActionEvent;
import com.intellij.openapi.project.Project;
import com.intellij.openapi.wm.ToolWindow;
import com.intellij.openapi.wm.ToolWindowManager;
import com.intellij.ui.jcef.JBCefApp;

/** Shows the cargo-arc tool window and opens or re-shows its page. */
public final class OpenAction extends AnAction {

    @Override
    public void actionPerformed(AnActionEvent event) {
        Project project = event.getProject();
        if (project == null) {
            return;
        }
        if (!JBCefApp.isSupported()) {
            // The tool window's factory shows the "JCEF is unavailable" label
            // instead of a browser; starting the service would run it headless.
            return;
        }
        ToolWindow toolWindow = ToolWindowManager.getInstance(project).getToolWindow("cargo-arc");
        if (toolWindow == null) {
            return;
        }
        ArcService service = project.getService(ArcService.class);
        toolWindow.activate(service::open);
    }

    @Override
    public void update(AnActionEvent event) {
        event.getPresentation().setEnabled(event.getProject() != null);
    }

    @Override
    public ActionUpdateThread getActionUpdateThread() {
        return ActionUpdateThread.BGT;
    }
}
