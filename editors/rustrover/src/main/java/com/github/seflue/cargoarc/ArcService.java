package com.github.seflue.cargoarc;

import com.intellij.notification.NotificationGroupManager;
import com.intellij.notification.NotificationType;
import com.intellij.openapi.Disposable;
import com.intellij.openapi.components.Service;
import com.intellij.openapi.project.Project;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.ArrayDeque;
import java.util.Deque;
import java.util.List;
import java.util.Optional;
import java.util.function.Consumer;
import java.util.function.Supplier;

/**
 * Runs the {@code cargo-arc ui} service as a child process, shows its page
 * once it announces a port, and forwards its jump requests to the navigator.
 *
 * <p>The package-private constructor takes a working directory and command
 * supplier instead of a {@code Project}, so a test can run a fake process
 * without an IDE. The platform constructs an instance with the public
 * {@code Project} constructor instead.
 */
@Service(Service.Level.PROJECT)
public final class ArcService implements Disposable {

    // Bounded so a service that fails long after starting never grows the
    // buffer without limit.
    private static final int STDERR_TAIL_LINES = 50;

    private final Supplier<List<String>> command;
    private final Path workingDirectory;
    private final Navigator navigator;
    private final Consumer<String> onFailure;

    // Not final: the platform constructor sets a placeholder here and the
    // tool window replaces it via setPageHost once its browser exists.
    private volatile PageHost pageHost;

    private final Object lock = new Object();
    private Process process;
    private String version;
    private Integer port;
    private boolean stopRequested;
    private boolean restartRequested;
    private final Deque<String> stderrTail = new ArrayDeque<>();

    ArcService(
        Supplier<List<String>> command,
        Path workingDirectory,
        Navigator navigator,
        PageHost pageHost,
        Consumer<String> onFailure
    ) {
        this.command = command;
        this.workingDirectory = workingDirectory;
        this.navigator = navigator;
        this.pageHost = pageHost;
        this.onFailure = onFailure;
    }

    /**
     * Wires the navigator and failure notifications to {@code project}, with
     * a launch command read from {@link ArcApplicationSettings} and
     * {@link ArcProjectSettings} on every start, so a setting changed before
     * a restart takes effect. The page host starts out as a no-op; the tool
     * window supplies the real one through {@link #setPageHost} once its
     * browser exists.
     */
    public ArcService(Project project) {
        this(
            () -> launchOptions(project).argv(),
            Path.of(project.getBasePath()),
            new OpenFileDescriptorNavigator(project),
            url -> { },
            message -> NotificationGroupManager.getInstance()
                .getNotificationGroup("cargo-arc")
                .createNotification(message, NotificationType.ERROR)
                .notify(project)
        );
    }

    private static LaunchOptions launchOptions(Project project) {
        ArcApplicationSettings applicationSettings = ArcApplicationSettings.getInstance();
        ArcProjectSettings projectSettings = ArcProjectSettings.getInstance(project);
        String manifestPath = projectSettings.manifestPath();
        return new LaunchOptions(
            applicationSettings.binary(),
            manifestPath.isEmpty() ? null : manifestPath,
            projectSettings.featureList(),
            projectSettings.includeTests(),
            projectSettings.externals()
        );
    }

    /** Replaces the page host, once the tool window has created its browser. */
    public void setPageHost(PageHost pageHost) {
        this.pageHost = pageHost;
    }

    /**
     * Shows the page of a running, announced service again; starts one when
     * none runs. Does nothing while a process runs but has not announced a
     * port yet.
     */
    public void open() {
        boolean alive;
        Integer announcedPort;
        synchronized (lock) {
            alive = process != null && process.isAlive();
            announcedPort = port;
        }
        if (alive) {
            if (announcedPort != null) {
                pageHost.show(url(announcedPort));
            }
            return;
        }
        start();
    }

    /**
     * Ends the running service and starts a new one once it has exited, so
     * the page can reload against the new process. Starts one directly when
     * none runs.
     */
    public void restart() {
        Process toDestroy = null;
        synchronized (lock) {
            if (process != null && process.isAlive()) {
                restartRequested = true;
                stopRequested = true;
                toDestroy = process;
            }
        }
        if (toDestroy == null) {
            start();
        } else {
            toDestroy.destroy();
        }
    }

    /** Ends the service. The exit watcher clears the running state. */
    public void stop() {
        Process toDestroy;
        synchronized (lock) {
            if (process == null || !process.isAlive()) {
                return;
            }
            stopRequested = true;
            restartRequested = false;
            toDestroy = process;
        }
        toDestroy.destroy();
    }

    public boolean isRunning() {
        synchronized (lock) {
            return process != null && process.isAlive();
        }
    }

    public Optional<Status> status() {
        synchronized (lock) {
            if (process == null || port == null) {
                return Optional.empty();
            }
            return Optional.of(new Status(version, port, process.pid()));
        }
    }

    @Override
    public void dispose() {
        Process toDestroy;
        synchronized (lock) {
            stopRequested = true;
            restartRequested = false;
            toDestroy = process;
        }
        if (toDestroy != null) {
            toDestroy.destroy();
        }
    }

    private void start() {
        ProcessBuilder builder = new ProcessBuilder(command.get());
        builder.directory(workingDirectory.toFile());
        Process started;
        try {
            started = builder.start();
        } catch (IOException failure) {
            onFailure.accept("cargo-arc: cannot start: " + failure.getMessage());
            return;
        }
        synchronized (lock) {
            process = started;
            version = null;
            port = null;
            stopRequested = false;
            restartRequested = false;
            stderrTail.clear();
        }
        readLines(started.getInputStream(), this::handleLine);
        Thread stderrReader = readLines(started.getErrorStream(), this::collectStderr);
        awaitExit(started, stderrReader);
    }

    private void handleLine(String line) {
        ServiceLine.parse(line).ifPresent(this::apply);
    }

    private void apply(ServiceLine parsed) {
        switch (parsed) {
            case ServiceLine.Ready ready -> onReady(ready);
            case ServiceLine.Jump jump -> navigator.jump(jump.file(), jump.line());
        }
    }

    private void onReady(ServiceLine.Ready ready) {
        synchronized (lock) {
            version = ready.version();
            port = ready.port();
        }
        pageHost.show(url(ready.port()));
    }

    private void collectStderr(String line) {
        synchronized (lock) {
            stderrTail.addLast(line);
            if (stderrTail.size() > STDERR_TAIL_LINES) {
                stderrTail.removeFirst();
            }
        }
    }

    private void awaitExit(Process started, Thread stderrReader) {
        Thread waiter = new Thread(() -> {
            int exitCode;
            try {
                exitCode = started.waitFor();
            } catch (InterruptedException interrupted) {
                Thread.currentThread().interrupt();
                return;
            }
            try {
                // Bounded: the reader drains a closed pipe almost immediately;
                // this only guards against never composing the tail at all.
                stderrReader.join(1000);
            } catch (InterruptedException interrupted) {
                Thread.currentThread().interrupt();
            }
            boolean announced;
            boolean stopped;
            boolean shouldRestart;
            String tail;
            synchronized (lock) {
                announced = port != null;
                stopped = stopRequested;
                shouldRestart = restartRequested;
                tail = String.join("\n", stderrTail);
                if (process == started) {
                    process = null;
                }
            }
            if (!announced && !stopped) {
                onFailure.accept(
                    "cargo-arc ended with exit code " + exitCode + " before announcing a port\n" + tail
                );
            }
            if (shouldRestart) {
                start();
            }
        }, "cargo-arc exit watcher");
        waiter.setDaemon(true);
        waiter.start();
    }

    private Thread readLines(InputStream stream, Consumer<String> sink) {
        Thread reader = new Thread(() -> {
            try (BufferedReader lines = new BufferedReader(new InputStreamReader(stream, StandardCharsets.UTF_8))) {
                for (String line = lines.readLine(); line != null; line = lines.readLine()) {
                    sink.accept(line);
                }
            } catch (IOException closed) {
                // Stream closed by destroy(); any other read error has no consumer here.
            }
        }, "cargo-arc reader");
        reader.setDaemon(true);
        reader.start();
        return reader;
    }

    private static String url(int port) {
        return "http://127.0.0.1:" + port + "/";
    }

    /** Version, port and pid of a service that has announced itself. */
    public record Status(String version, int port, long pid) {}
}
