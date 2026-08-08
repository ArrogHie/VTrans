cmake_minimum_required(VERSION 3.20)
project(translation_bridge LANGUAGES CXX)

# C++17 bridge around Bergamot v0.4.5 + CTranslate2 4.8.1 + SentencePiece.
# The three engines are expected to be built separately and their include /
# library paths are passed in through the cache variables below (see
# build.ps1 and README.md for the exact layout).
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

if(NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE Release CACHE STRING "Build type" FORCE)
endif()

add_library(translation_bridge SHARED
    translation_bridge.cpp
    translation_bridge.h
)

target_include_directories(translation_bridge PRIVATE
    ${BERGAMOT_INCLUDE_DIRS}
    ${CTRANSLATE2_INCLUDE_DIRS}
    ${SENTENCEPIECE_INCLUDE_DIRS}
)

target_link_libraries(translation_bridge PRIVATE
    ${BERGAMOT_LIBRARIES}
    ${CTRANSLATE2_LIBRARIES}
    ${SENTENCEPIECE_LIBRARIES}
)

if(WIN32)
    target_compile_definitions(translation_bridge PRIVATE
        _CRT_SECURE_NO_WARNINGS
        NOMINMAX
    )
endif()

# Copy the produced DLL to the app resource directory when requested.
set(TRANSLATION_BRIDGE_OUTPUT_DIR "" CACHE PATH
    "Directory that receives translation_bridge.dll (default: src-tauri/resources/native)")

if(TRANSLATION_BRIDGE_OUTPUT_DIR)
    add_custom_command(TARGET translation_bridge POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E make_directory
                "${TRANSLATION_BRIDGE_OUTPUT_DIR}"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different
                "$<TARGET_FILE:translation_bridge>"
                "${TRANSLATION_BRIDGE_OUTPUT_DIR}"
        COMMENT "Copying translation_bridge to ${TRANSLATION_BRIDGE_OUTPUT_DIR}"
    )
endif()
