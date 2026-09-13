import org.jetbrains.intellij.platform.gradle.IntelliJPlatformType

plugins {
    id("java")
    id("org.jetbrains.intellij.platform") version "2.18.1"
}

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    val rustRoverPath = providers.gradleProperty("rustRoverPath").orNull

    intellijPlatform {
        if (rustRoverPath != null) {
            local(rustRoverPath)
        } else {
            create(IntelliJPlatformType.RustRover, providers.gradleProperty("platformVersion"))
        }
    }

    testImplementation("org.junit.jupiter:junit-jupiter:5.14.4")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

intellijPlatform {
    // Instrumentation pulls java-compiler-ant-tasks for this exact build, and
    // that artifact is not published for every build the Toolbox ships. Nothing
    // here needs it: no GUI forms, no annotation-driven null checks.
    instrumentCode = false

    pluginConfiguration {
        ideaVersion {
            sinceBuild = "251"
        }
    }
}

tasks {
    withType<JavaCompile> {
        options.release.set(21)
    }

    test {
        useJUnitPlatform()
    }
}
