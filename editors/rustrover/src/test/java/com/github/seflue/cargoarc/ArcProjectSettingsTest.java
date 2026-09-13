package com.github.seflue.cargoarc;

import org.junit.jupiter.api.Test;

import java.util.List;

import static org.junit.jupiter.api.Assertions.assertEquals;

class ArcProjectSettingsTest {

    @Test
    void emptyStringYieldsNoFeatures() {
        assertEquals(List.of(), ArcProjectSettings.parseFeatures(""));
    }

    @Test
    void commaSeparatedFeaturesSplit() {
        assertEquals(List.of("a", "b"), ArcProjectSettings.parseFeatures("a,b"));
    }

    @Test
    void surroundingSpacesAreTrimmed() {
        assertEquals(List.of("a", "b"), ArcProjectSettings.parseFeatures(" a , b "));
    }

    @Test
    void strayCommasDoNotProduceBlankFeatures() {
        assertEquals(List.of("a", "b"), ArcProjectSettings.parseFeatures("a,,b,"));
    }
}
