package com.github.seflue.cargoarc;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;

class OpenFileDescriptorNavigatorTest {

    @Test
    void clampsLineBeyondTheEndToTheLastLine() {
        assertEquals(10, OpenFileDescriptorNavigator.clampLine(15, 10));
    }

    @Test
    void clampsZeroOrNegativeToTheFirstLine() {
        assertEquals(1, OpenFileDescriptorNavigator.clampLine(0, 10));
        assertEquals(1, OpenFileDescriptorNavigator.clampLine(-3, 10));
    }

    @Test
    void leavesAnInRangeLineUnchanged() {
        assertEquals(5, OpenFileDescriptorNavigator.clampLine(5, 10));
    }
}
