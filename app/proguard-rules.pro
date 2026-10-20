# Mantém as classes usadas via JNI.
-keep class com.deivid.pkgtool.core.NativeCore { *; }
-keep class com.deivid.pkgtool.core.NativeProgress { *; }
-keepclasseswithmembernames class * {
    native <methods>;
}
