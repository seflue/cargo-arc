package com.github.seflue.cargoarc;

import org.junit.jupiter.api.Test;

import java.util.List;

import static org.junit.jupiter.api.Assertions.assertEquals;

class LaunchOptionsTest {

    @Test
    void minimalArgv() {
        LaunchOptions options = new LaunchOptions("cargo-arc", null, List.of(), false, false);

        assertEquals(List.of("cargo-arc", "arc", "ui"), options.argv());
    }

    @Test
    void everyOptionSetJoinsFeaturesAndKeepsOrder() {
        LaunchOptions options =
            new LaunchOptions("cargo-arc", "/repo/Cargo.toml", List.of("a", "b"), true, true);

        assertEquals(
            List.of(
                "cargo-arc", "arc",
                "--manifest-path", "/repo/Cargo.toml",
                "--features", "a,b",
                "--include-tests",
                "--externals",
                "ui"
            ),
            options.argv()
        );
    }
}
