package com.github.seflue.cargoarc;

import org.junit.jupiter.api.Test;

import java.nio.file.Path;
import java.util.Optional;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

class ServiceLineTest {

    @Test
    void parsesReadyLine() {
        assertEquals(
            Optional.of(new ServiceLine.Ready("0.4.0", 4321)),
            ServiceLine.parse("arc ready 0.4.0 4321")
        );
    }

    @Test
    void parsesJumpLine() {
        assertEquals(
            Optional.of(new ServiceLine.Jump(7, Path.of("/tmp/lib.rs"))),
            ServiceLine.parse("arc jump 7 /tmp/lib.rs")
        );
    }

    @Test
    void parsesJumpLineWithSpacesInPath() {
        assertEquals(
            Optional.of(new ServiceLine.Jump(3, Path.of("/tmp/a b/lib.rs"))),
            ServiceLine.parse("arc jump 3 /tmp/a b/lib.rs")
        );
    }

    @Test
    void rejectsTrailingGarbage() {
        assertTrue(ServiceLine.parse("arc ready 0.4.0 4321 extra").isEmpty());
    }

    @Test
    void rejectsUnrelatedLine() {
        assertTrue(ServiceLine.parse("something else entirely").isEmpty());
    }
}
