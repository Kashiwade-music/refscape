cmake_minimum_required(VERSION 3.20)
project(refscape_cpp_demo LANGUAGES C CXX)

set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
add_library(refscape_demo STATIC src/scale.c src/entry.cpp)
target_include_directories(refscape_demo PUBLIC include)
target_compile_definitions(refscape_demo PRIVATE REFSCAPE_DEMO_FACTOR=3)
target_compile_features(refscape_demo PUBLIC c_std_11 cxx_std_17)

add_executable(refscape_cpp_demo src/main.cpp)
target_link_libraries(refscape_cpp_demo PRIVATE refscape_demo)
