import java.io.File
import org.gradle.testfixtures.ProjectBuilder
import org.gradle.testkit.runner.GradleRunner
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class BuildTaskTest {
    @get:Rule val temporary = TemporaryFolder()

    @Test fun runnerUsesNodeCliAndPreservesTargetArguments() {
        val root = temporary.newFolder("node runner fixture")
        val app = File(root, "app").apply { mkdirs() }
        val cli = File(root, "node_modules/@tauri-apps/cli/tauri.js")
        cli.parentFile.mkdirs()
        cli.writeText("require('fs').writeFileSync('arguments.json', JSON.stringify(process.argv.slice(2)))")
        val project = ProjectBuilder.builder().withProjectDir(app).build()
        val task = project.tasks.create("nativeFixture", BuildTask::class.java)
        task.rootDirRel = ".."
        task.target = "aarch64"
        task.release = true
        task.assemble()
        assertEquals("[\"android\",\"android-studio-script\",\"--release\",\"--target\",\"aarch64\"]", File(root, "arguments.json").readText())
    }

    @Test fun debugConfigDoesNotRequireSigningProperties() {
        val source = File(System.getProperty("user.dir")).parentFile
        val fixture = temporary.newFolder("unsigned debug")
        val app = File(fixture, "app").apply { mkdirs() }
        File(fixture, "settings.gradle").writeText("rootProject.name = 'debug-fixture'\ninclude ':app'\n")
        File(source, "build.gradle.kts").copyTo(File(fixture, "build.gradle.kts"))
        File(source, "app/build.gradle.kts").copyTo(File(app, "build.gradle.kts"))
        File(app, "tauri.build.gradle.kts").writeText("")
        File(app, "src/main").mkdirs()
        File(app, "src/main/AndroidManifest.xml").writeText("<manifest />")
        val buildSrc = File(fixture, "buildSrc").apply { mkdirs() }
        File(source, "buildSrc/build.gradle.kts").copyTo(File(buildSrc, "build.gradle.kts"))
        File(source, "buildSrc/src/main").copyRecursively(File(buildSrc, "src/main"))
        assertFalse(File(fixture, "key.properties").exists())
        val result = GradleRunner.create().withProjectDir(fixture)
            .withArguments(":app:tasks", "--all", "--stacktrace").build()
        assertTrue(result.output.contains("assembleDebug"))
    }
}
