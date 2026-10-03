import java.io.File
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.logging.LogLevel
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.TaskAction

open class BuildTask : DefaultTask() {
    @Input var rootDirRel: String? = null
    @Input var target: String? = null
    @Input var release: Boolean? = null

    @TaskAction
    fun assemble() {
        val root = File(project.projectDir, rootDirRel ?: throw GradleException("rootDirRel cannot be null"))
        val target = target ?: throw GradleException("target cannot be null")
        val release = release ?: throw GradleException("release cannot be null")
        val cli = File(root, "node_modules/@tauri-apps/cli/tauri.js")
        if (!cli.isFile) throw GradleException("Install the project Node dependencies before building Android.")
        // The Tauri CLI owns the Android Studio options server for this call.
        // Never retry a failed build as another executable or silently use cargo-tauri.
        project.exec {
            workingDir(root)
            executable("node")
            args(cli.absolutePath, "android", "android-studio-script")
            if (project.logger.isEnabled(LogLevel.DEBUG)) args("-vv")
            else if (project.logger.isEnabled(LogLevel.INFO)) args("-v")
            if (release) args("--release")
            args("--target", target)
        }.assertNormalExitValue()
    }
}