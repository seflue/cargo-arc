package com.github.seflue.cargoarc;

import com.intellij.openapi.actionSystem.ActionManager;
import com.intellij.openapi.application.ApplicationManager;
import com.intellij.openapi.project.DumbAware;
import com.intellij.openapi.project.Project;
import com.intellij.openapi.util.Disposer;
import com.intellij.openapi.wm.ToolWindow;
import com.intellij.openapi.wm.ToolWindowFactory;
import com.intellij.ui.content.Content;
import com.intellij.ui.content.ContentFactory;
import com.intellij.ui.jcef.JBCefApp;
import com.intellij.ui.jcef.JBCefBrowser;

import javax.swing.JComponent;
import javax.swing.JLabel;
import java.util.List;

/** Builds the tool window's browser and starts the service against it. */
public final class ArcToolWindowFactory implements ToolWindowFactory, DumbAware {

    @Override
    public void createToolWindowContent(Project project, ToolWindow toolWindow) {
        if (!JBCefApp.isSupported()) {
            addContent(toolWindow, new JLabel("JCEF is unavailable in this IDE installation."));
            return;
        }

        JBCefBrowser browser = new JBCefBrowser();
        Disposer.register(toolWindow.getDisposable(), browser);
        addContent(toolWindow, browser.getComponent());
        ActionManager actionManager = ActionManager.getInstance();
        toolWindow.setTitleActions(List.of(
            actionManager.getAction("com.github.seflue.cargoarc.RestartAction"),
            actionManager.getAction("com.github.seflue.cargoarc.StopAction")
        ));

        ArcService service = project.getService(ArcService.class);
        service.setPageHost(url -> ApplicationManager.getApplication().invokeLater(() -> browser.loadURL(url)));
        service.open();
    }

    private void addContent(ToolWindow toolWindow, JComponent component) {
        Content content = ContentFactory.getInstance().createContent(component, "", false);
        toolWindow.getContentManager().addContent(content);
    }
}
