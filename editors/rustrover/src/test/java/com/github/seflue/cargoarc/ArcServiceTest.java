package com.github.seflue.cargoarc;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.condition.EnabledOnOs;
import org.junit.jupiter.api.condition.OS;

import java.nio.file.Path;
import java.time.Duration;
import java.time.Instant;
import java.util.List;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.BooleanSupplier;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

@EnabledOnOs(OS.LINUX)
class ArcServiceTest {

    private static final String FAKE_SERVICE =
        "echo 'arc ready 0.0.0 4321'; echo 'arc jump 7 /tmp/a b/lib.rs'; sleep 30";

    @Test
    void announcesPortAndForwardsJumpsThenStops() throws InterruptedException {
        AtomicReference<String> shownUrl = new AtomicReference<>();
        AtomicReference<Path> jumpedFile = new AtomicReference<>();
        AtomicReference<Integer> jumpedLine = new AtomicReference<>();
        AtomicReference<String> failureMessage = new AtomicReference<>();

        ArcService service = new ArcService(
            () -> List.of("sh", "-c", FAKE_SERVICE),
            Path.of(System.getProperty("user.dir")),
            (file, line) -> {
                jumpedFile.set(file);
                jumpedLine.set(line);
            },
            shownUrl::set,
            failureMessage::set
        );

        service.open();
        waitUntil(() -> jumpedFile.get() != null);

        assertEquals("http://127.0.0.1:4321/", shownUrl.get());
        assertEquals(Path.of("/tmp/a b/lib.rs"), jumpedFile.get());
        assertEquals(7, jumpedLine.get());
        assertTrue(service.isRunning());

        service.stop();
        waitUntil(() -> !service.isRunning());
        assertFalse(service.isRunning());
        assertNull(failureMessage.get());
    }

    @Test
    void stopCancelsAPendingRestart() throws InterruptedException {
        ArcService service = new ArcService(
            () -> List.of("sh", "-c", "sleep 30"),
            Path.of(System.getProperty("user.dir")),
            (file, line) -> { },
            url -> { },
            failure -> { }
        );

        service.open();
        waitUntil(service::isRunning);

        service.restart();
        service.stop();

        waitUntil(() -> !service.isRunning());
        Thread.sleep(200);
        assertFalse(service.isRunning());
    }

    @Test
    void disposeCancelsAPendingRestart() throws InterruptedException {
        ArcService service = new ArcService(
            () -> List.of("sh", "-c", "sleep 30"),
            Path.of(System.getProperty("user.dir")),
            (file, line) -> { },
            url -> { },
            failure -> { }
        );

        service.open();
        waitUntil(service::isRunning);

        service.restart();
        service.dispose();

        waitUntil(() -> !service.isRunning());
        Thread.sleep(200);
        assertFalse(service.isRunning());
    }

    private static void waitUntil(BooleanSupplier condition) throws InterruptedException {
        Instant deadline = Instant.now().plus(Duration.ofSeconds(5));
        while (Instant.now().isBefore(deadline)) {
            if (condition.getAsBoolean()) {
                return;
            }
            Thread.sleep(20);
        }
        throw new AssertionError("condition not met within 5s");
    }
}
