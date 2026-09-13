package com.github.seflue.cargoarc;

import com.intellij.notification.NotificationGroupManager;
import com.intellij.notification.NotificationType;
import com.intellij.openapi.application.ApplicationManager;
import com.intellij.openapi.editor.Document;
import com.intellij.openapi.fileEditor.FileDocumentManager;
import com.intellij.openapi.fileEditor.OpenFileDescriptor;
import com.intellij.openapi.project.Project;
import com.intellij.openapi.vfs.LocalFileSystem;
import com.intellij.openapi.vfs.VirtualFile;

import java.nio.file.Path;

/** Puts the cursor on a jump the service reported. */
public final class OpenFileDescriptorNavigator implements Navigator {

    private static final String NOTIFICATION_GROUP = "cargo-arc";

    private final Project project;

    public OpenFileDescriptorNavigator(Project project) {
        this.project = project;
    }

    @Override
    public void jump(Path file, int line) {
        ApplicationManager.getApplication().invokeLater(() -> navigate(file, line));
    }

    private void navigate(Path file, int line) {
        VirtualFile virtualFile = LocalFileSystem.getInstance().refreshAndFindFileByNioFile(file);
        if (virtualFile == null) {
            NotificationGroupManager.getInstance()
                .getNotificationGroup(NOTIFICATION_GROUP)
                .createNotification("cargo-arc: no such file: " + file, NotificationType.ERROR)
                .notify(project);
            return;
        }
        int clamped = clampLine(line, lineCount(virtualFile));
        new OpenFileDescriptor(project, virtualFile, clamped - 1, 0).navigate(true);
    }

    private int lineCount(VirtualFile file) {
        Document document = FileDocumentManager.getInstance().getDocument(file);
        return document != null ? document.getLineCount() : 1;
    }

    /**
     * Clamps a one-based line to a document of {@code lineCount} lines: below
     * one becomes one, past the end becomes the last line.
     */
    static int clampLine(int line, int lineCount) {
        int last = Math.max(lineCount, 1);
        return Math.max(1, Math.min(line, last));
    }
}
