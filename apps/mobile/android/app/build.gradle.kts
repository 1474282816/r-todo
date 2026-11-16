plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

// 互传 Rust 产出的 ABI；与 Flutter 默认打包的 ABI 保持一致（可用 -PrtodoAbis=... 覆盖）。
val rtodoAbis: List<String> = ((findProperty("rtodoAbis") as String?)
    ?: "arm64-v8a,armeabi-v7a,x86_64")
    .split(',')
    .map { it.trim() }
    .filter { it.isNotEmpty() }

android {
    namespace = "dev.rtodo.r_todo_mobile"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    compileOptions {
        isCoreLibraryDesugaringEnabled = true
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        // TODO: Specify your own unique Application ID (https://developer.android.com/studio/build/application-id.html).
        applicationId = "dev.rtodo.r_todo_mobile"
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        minSdk = flutter.minSdkVersion
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName

        // 与 Rust 产物保持一致，避免装到缺少 librtodo_mobile_transfer.so 的 ABI 上闪退。
        // --split-per-abi 时 Flutter 自己会设 splits.abi，两者不能同时配置。
        if (project.findProperty("split-per-abi")?.toString() != "true") {
            ndk {
                abiFilters += rtodoAbis
            }
        }
    }

    buildTypes {
        release {
            // 个人自用构建：可通过环境变量提供独立签名，否则回退到本机 debug key。
            val storePath = System.getenv("RTOOLS_KEYSTORE")
            if (!storePath.isNullOrBlank()) {
                signingConfig = signingConfigs.create("rtoolsRelease") {
                    storeFile = file(storePath)
                    storePassword = System.getenv("RTOOLS_STORE_PASSWORD")
                    keyAlias = System.getenv("RTOOLS_KEY_ALIAS")
                    keyPassword = System.getenv("RTOOLS_KEY_PASSWORD")
                }
            } else {
                signingConfig = signingConfigs.getByName("debug")
            }
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}

dependencies {
    // flutter_local_notifications 需要 core library desugaring
    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.4")
    // 互传前台服务 / 通知 / 权限（NotificationCompat、ServiceCompat、ContextCompat）
    implementation("androidx.core:core-ktx:1.13.1")
}

// ---- Rust / FRB ----
// 用 cargo-ndk 直接产出 jniLibs，不引入 cargokit（AGP 9 / Gradle 9 兼容性未知）。
val rtodoRustDir = rootProject.file("../rust")
val rtodoJniLibsDir = file("src/main/jniLibs")
// 打 release 包时自动切 Rust release（也可用 -PrtodoRustProfile=debug 覆盖）
val rtodoRustProfile: String = (findProperty("rtodoRustProfile") as String?)
    ?: if (gradle.startParameter.taskNames.any {
            it.contains("Release", ignoreCase = true)
        }) {
        "release"
    } else {
        "debug"
    }

val buildRustLibs by tasks.registering(Exec::class) {
    group = "rust"
    description = "Build rtodo_mobile_transfer for Android ABIs into jniLibs"
    workingDir = rtodoRustDir

    val args = mutableListOf("cargo", "ndk")
    rtodoAbis.forEach { args += listOf("-t", it) }
    args += listOf("-o", rtodoJniLibsDir.absolutePath, "build")
    if (rtodoRustProfile == "release") args += "--release"
    commandLine(args)

    System.getenv("ANDROID_NDK_HOME")?.takeIf { it.isNotBlank() }?.let {
        environment("ANDROID_NDK_HOME", it)
    }

    inputs.dir(rtodoRustDir.resolve("src"))
    inputs.file(rtodoRustDir.resolve("Cargo.toml"))
    inputs.property("abis", rtodoAbis.joinToString(","))
    inputs.property("profile", rtodoRustProfile)
    outputs.dir(rtodoJniLibsDir)
}

tasks.named("preBuild").configure {
    dependsOn(buildRustLibs)
}
