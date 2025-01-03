.DEFAULT_GOAL := apk
.PHONY: jni apk run-on-device

gradle:
	cd java && gradle wrapper

jni:
	# cargo ndk --target arm64-v8a -o ./java/app/src/main/jniLibs/ build --profile release
	# cargo ndk --target x86_64-linux-android -o ./java/app/src/main/jniLibs/ build --profile release
	cargo ndk --target aarch64-linux-android -o ./java/app/src/main/jniLibs/ build --profile release

jgd:
	cd java && ./gradlew build --stacktrace

apk: jni
	cd java && ./gradlew build --stacktrace

run-on-device: jni
	adb uninstall co.p4ymak.roomor || true

	cd java && ./gradlew installDebug
	adb shell am start -n co.p4ymak.roomor/.MainActivity
	adb logcat -v color -s roomor *:e

clean:
	rm -rf java/app/src/main/jniLibs/
	rm -rf java/app/build/
	rm -rf java/.gradle/
	rm -rf target/
	rm -rf dist/
