#include <iostream>
#include <fstream>
#include <vector>
#include <map>
#include <memory>
#include <variant>
#include <chrono>
#include <thread>
#include <nlohmann/json.hpp>

using json = nlohmann::json;

// Forward declarations
struct Expression;
using ExpressionPtr = std::shared_ptr<Expression>;

// Expression types
struct Expression {
    virtual ~Expression() = default;
    virtual uint64_t evaluate(const std::map<std::string, uint64_t>& vars) const = 0;
};

struct VarExpression : Expression {
    std::string name;
    VarExpression(const std::string& n) : name(n) {}
    uint64_t evaluate(const std::map<std::string, uint64_t>& vars) const override {
        auto it = vars.find(name);
        return it != vars.end() ? it->second : 0;
    }
};

struct ConstExpression : Expression {
    uint64_t value;
    ConstExpression(uint64_t v) : value(v) {}
    uint64_t evaluate(const std::map<std::string, uint64_t>&) const override {
        return value;
    }
};

struct MulExpression : Expression {
    ExpressionPtr left, right;
    MulExpression(ExpressionPtr l, ExpressionPtr r) : left(l), right(r) {}
    uint64_t evaluate(const std::map<std::string, uint64_t>& vars) const override {
        return left->evaluate(vars) * right->evaluate(vars);
    }
};

struct SubExpression : Expression {
    ExpressionPtr left, right;
    SubExpression(ExpressionPtr l, ExpressionPtr r) : left(l), right(r) {}
    uint64_t evaluate(const std::map<std::string, uint64_t>& vars) const override {
        return left->evaluate(vars) - right->evaluate(vars);
    }
};

struct LeExpression : Expression {
    ExpressionPtr left, right;
    LeExpression(ExpressionPtr l, ExpressionPtr r) : left(l), right(r) {}
    uint64_t evaluate(const std::map<std::string, uint64_t>& vars) const override {
        return left->evaluate(vars) <= right->evaluate(vars) ? 1 : 0;
    }
};

// Parse expression from JSON
ExpressionPtr parseExpression(const json& j) {
    if (j.contains("var")) {
        return std::make_shared<VarExpression>(j["var"]);
    } else if (j.contains("const")) {
        return std::make_shared<ConstExpression>(j["const"]);
    } else if (j.contains("mul")) {
        auto& mul = j["mul"];
        return std::make_shared<MulExpression>(
            parseExpression(mul[0]), parseExpression(mul[1]));
    } else if (j.contains("sub")) {
        auto& sub = j["sub"];
        return std::make_shared<SubExpression>(
            parseExpression(sub[0]), parseExpression(sub[1]));
    } else if (j.contains("le")) {
        auto& le = j["le"];
        return std::make_shared<LeExpression>(
            parseExpression(le[0]), parseExpression(le[1]));
    }
    return std::make_shared<ConstExpression>(0);
}

// Stack entry types
struct StateEntry {
    std::string state;
};

struct RetrnEntry {
    std::string state;
};

struct ValueEntry {
    std::string type;
    uint64_t value;
};

using StackEntry = std::variant<StateEntry, RetrnEntry, ValueEntry>;

// Maroon VM
class MaroonVM {
private:
    json program;
    std::vector<StackEntry> stack;
    std::map<std::string, uint64_t> localVars;
    uint64_t currentTime = 0;

    std::string interpolateText(const std::string& text) {
        std::string result = text;
        for (const auto& [var, value] : localVars) {
            std::string placeholder = "{" + var + "}";
            size_t pos = result.find(placeholder);
            if (pos != std::string::npos) {
                result.replace(pos, placeholder.length(), std::to_string(value));
            }
        }
        return result;
    }

public:
    MaroonVM(const json& prog) : program(prog) {}

    void execute(uint64_t initialArg) {
        auto& functions = program["functions"];
        auto& entryFunc = functions[program["entry_point"]];
        
        // Initialize stack
        localVars["n"] = initialArg;
        stack.push_back(ValueEntry{"FactorialInput", initialArg});
        stack.push_back(StateEntry{"FactorialEntry"});

        while (!stack.empty()) {
            // Pop current state
            auto currentEntry = stack.back();
            stack.pop_back();
            
            if (!std::holds_alternative<StateEntry>(currentEntry)) {
                throw std::runtime_error("Expected state on top of stack");
            }
            
            std::string currentStateName = std::get<StateEntry>(currentEntry).state;

            // Find state definition
            json stateDef;
            for (auto& state : entryFunc["states"]) {
                if (state["name"] == currentStateName) {
                    stateDef = state;
                    break;
                }
            }

            // Pop local variables
            std::vector<uint64_t> localValues;
            for (size_t i = 0; i < stateDef["local_vars"]; i++) {
                if (!stack.empty() && std::holds_alternative<ValueEntry>(stack.back())) {
                    localValues.push_back(std::get<ValueEntry>(stack.back()).value);
                    stack.pop_back();
                }
            }

            // Update local variables
            if (currentStateName.find("PostRecursiveCall") != std::string::npos && localValues.size() >= 2) {
                localVars["result"] = localValues[0];
                localVars["n"] = localValues[1];
            } else if ((currentStateName == "FactorialRecursiveCall" || 
                        currentStateName.find("Post") != std::string::npos ||
                        currentStateName.find("Argument") != std::string::npos) && !localValues.empty()) {
                localVars["n"] = localValues[0];
            }

            // Execute operations
            for (auto& op : stateDef["operations"]) {
                std::string opType = op["type"];

                if (opType == "push_stack") {
                    for (auto& entry : op["entries"]) {
                        if (entry.contains("State")) {
                            stack.push_back(StateEntry{entry["State"]});
                        } else if (entry.contains("Retrn")) {
                            stack.push_back(RetrnEntry{entry["Retrn"]});
                        } else if (entry.contains("Value")) {
                            auto& val = entry["Value"];
                            std::string valType = val.begin().key();
                            auto expr = parseExpression(val[valType]);
                            uint64_t value = expr->evaluate(localVars);
                            stack.push_back(ValueEntry{valType, value});
                        }
                    }
                    break; // Continue to next iteration of main loop
                } else if (opType == "write") {
                    std::string output = interpolateText(op["text"]);
                    std::cout << currentTime << "ms: " << output << std::endl;
                    
                    // Push local vars back
                    for (auto it = localValues.rbegin(); it != localValues.rend(); ++it) {
                        stack.push_back(ValueEntry{"FactorialArgument", *it});
                    }
                    stack.push_back(StateEntry{op["next_state"]});
                    break; // Exit the operations loop
                } else if (opType == "sleep") {
                    auto sleepExpr = parseExpression(op["ms"]);
                    uint64_t sleepMs = sleepExpr->evaluate(localVars);
                    std::this_thread::sleep_for(std::chrono::milliseconds(sleepMs));
                    currentTime += sleepMs;
                    
                    // Push local vars back
                    for (auto it = localValues.rbegin(); it != localValues.rend(); ++it) {
                        stack.push_back(ValueEntry{"FactorialArgument", *it});
                    }
                    stack.push_back(StateEntry{op["next_state"]});
                    break; // Exit the operations loop
                } else if (opType == "conditional") {
                    auto condExpr = parseExpression(op["condition"]);
                    uint64_t condValue = condExpr->evaluate(localVars);
                    auto& opsToExecute = condValue != 0 ? op["then"] : op["else"];
                    
                    bool pushedStack = false;
                    for (auto& execOp : opsToExecute) {
                        std::string execOpType = execOp["type"];
                        
                        if (execOpType == "return") {
                            auto retExpr = parseExpression(execOp["value"]);
                            uint64_t retValue = retExpr->evaluate(localVars);
                            
                            // Pop return address
                            if (!stack.empty() && std::holds_alternative<RetrnEntry>(stack.back())) {
                                auto retrn = std::get<RetrnEntry>(stack.back());
                                stack.pop_back();
                                stack.push_back(ValueEntry{"FactorialReturnValue", retValue});
                                stack.push_back(StateEntry{retrn.state});
                            }
                        } else if (execOpType == "push_stack") {
                            // First push back the local values to preserve them
                            for (auto it = localValues.rbegin(); it != localValues.rend(); ++it) {
                                stack.push_back(ValueEntry{"FactorialArgument", *it});
                            }
                            // Then push the new entries
                            for (auto& entry : execOp["entries"]) {
                                if (entry.contains("State")) {
                                    stack.push_back(StateEntry{entry["State"]});
                                } else if (entry.contains("Retrn")) {
                                    stack.push_back(RetrnEntry{entry["Retrn"]});
                                } else if (entry.contains("Value")) {
                                    auto& val = entry["Value"];
                                    std::string valType = val.begin().key();
                                    auto expr = parseExpression(val[valType]);
                                    uint64_t value = expr->evaluate(localVars);
                                    stack.push_back(ValueEntry{valType, value});
                                }
                            }
                            pushedStack = true;
                        }
                    }
                    
                    if (pushedStack) {
                        break; // Exit the operations loop to continue main loop
                    }
                } else if (opType == "return") {
                    auto retExpr = parseExpression(op["value"]);
                    uint64_t retValue = retExpr->evaluate(localVars);
                    
                    // Pop return address
                    if (!stack.empty() && std::holds_alternative<RetrnEntry>(stack.back())) {
                        auto retrn = std::get<RetrnEntry>(stack.back());
                        stack.pop_back();
                        stack.push_back(ValueEntry{"FactorialReturnValue", retValue});
                        stack.push_back(StateEntry{retrn.state});
                    }
                } else if (opType == "done") {
                    if (localValues.size() >= 2) {
                        std::cout << currentTime << "ms: " << initialArg << "!=" << localValues[0] << std::endl;
                    }
                    return;
                }
            }
        }
    }
};

int main(int argc, char* argv[]) {
    if (argc != 3) {
        std::cerr << "Usage: " << argv[0] << " <bytecode.json> <n>" << std::endl;
        return 1;
    }

    try {
        // Read JSON bytecode
        std::ifstream file(argv[1]);
        if (!file.is_open()) {
            throw std::runtime_error("Failed to open bytecode file");
        }
        json program;
        file >> program;

        uint64_t n = std::stoull(argv[2]);

        // Execute
        MaroonVM vm(program);
        vm.execute(n);

    } catch (const std::exception& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }

    return 0;
}