# Added by the bsdev image: sshd doesn't propagate the image's ENV into login
# shells, so the Android SDK env + PATH must be set here (like the cargo snippet).
# Lives in /etc/fish/conf.d, which fish sources for every session.
set -gx ANDROID_SDK_ROOT /opt/android-sdk
set -gx ANDROID_HOME /opt/android-sdk
set -gx JAVA_HOME /usr/lib/jvm/java-21-openjdk
if test -d $ANDROID_SDK_ROOT/cmdline-tools/latest/bin
    fish_add_path -g $ANDROID_SDK_ROOT/cmdline-tools/latest/bin
end
if test -d $ANDROID_SDK_ROOT/platform-tools
    fish_add_path -g $ANDROID_SDK_ROOT/platform-tools
end
