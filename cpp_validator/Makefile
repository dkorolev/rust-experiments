CXX = g++
CXXFLAGS = -std=c++17 -Wall -O2
LIBS = -lnlohmann_json

TARGET = maroon_validator

all: $(TARGET)

$(TARGET): maroon_validator.cpp
	$(CXX) $(CXXFLAGS) -o $@ $< $(LIBS)

clean:
	rm -f $(TARGET)

test: $(TARGET)
	./$(TARGET) ../factorial_bytecode.json 5

.PHONY: all clean test