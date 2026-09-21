package sigil;

import java.io.File;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import org.eclipse.emf.common.util.EList;
import org.eclipse.emf.common.util.URI;
import org.eclipse.emf.ecore.EObject;
import org.eclipse.emf.ecore.resource.Resource;
import org.eclipse.emf.ecore.resource.ResourceSet;
import org.eclipse.xtext.EcoreUtil2;
import org.eclipse.xtext.resource.XtextResourceSet;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.regnosys.rosetta.builtin.RosettaBuiltinsService;
import com.regnosys.rosetta.rosetta.Import;
import com.regnosys.rosetta.rosetta.RosettaBasicType;
import com.regnosys.rosetta.rosetta.RosettaBody;
import com.regnosys.rosetta.rosetta.RosettaCardinality;
import com.regnosys.rosetta.rosetta.RosettaCorpus;
import com.regnosys.rosetta.rosetta.RosettaDocReference;
import com.regnosys.rosetta.rosetta.RosettaEnumValue;
import com.regnosys.rosetta.rosetta.RosettaEnumeration;
import com.regnosys.rosetta.rosetta.RosettaExternalFunction;
import com.regnosys.rosetta.rosetta.RosettaExternalRuleSource;
import com.regnosys.rosetta.rosetta.RosettaMetaType;
import com.regnosys.rosetta.rosetta.RosettaModel;
import com.regnosys.rosetta.rosetta.RosettaParameter;
import com.regnosys.rosetta.rosetta.RosettaQualifiableConfiguration;
import com.regnosys.rosetta.rosetta.RosettaRecordFeature;
import com.regnosys.rosetta.rosetta.RosettaRecordType;
import com.regnosys.rosetta.rosetta.RosettaReport;
import com.regnosys.rosetta.rosetta.RosettaRule;
import com.regnosys.rosetta.rosetta.RosettaSegment;
import com.regnosys.rosetta.rosetta.RosettaTypeAlias;
import com.regnosys.rosetta.rosetta.TypeCall;
import com.regnosys.rosetta.rosetta.TypeCallArgument;
import com.regnosys.rosetta.rosetta.TypeParameter;
import com.regnosys.rosetta.rosetta.simple.Annotation;
import com.regnosys.rosetta.rosetta.simple.AnnotationQualifier;
import com.regnosys.rosetta.rosetta.simple.AnnotationRef;
import com.regnosys.rosetta.rosetta.simple.Attribute;
import com.regnosys.rosetta.rosetta.simple.Choice;
import com.regnosys.rosetta.rosetta.simple.Data;
import com.regnosys.rosetta.rosetta.simple.LabelAnnotation;
import com.regnosys.rosetta.rosetta.simple.RuleReferenceAnnotation;
import com.regnosys.rosetta.rosetta.simple.Condition;
import com.regnosys.rosetta.rosetta.simple.Function;
import com.regnosys.rosetta.rosetta.simple.FunctionDispatch;
import com.regnosys.rosetta.rosetta.simple.Operation;
import com.regnosys.rosetta.rosetta.simple.Segment;
import com.regnosys.rosetta.rosetta.simple.ShortcutDeclaration;
import com.regnosys.rosetta.rosetta.expression.ArithmeticOperation;
import com.regnosys.rosetta.rosetta.expression.AsKeyOperation;
import com.regnosys.rosetta.rosetta.expression.ChoiceOperation;
import com.regnosys.rosetta.rosetta.expression.ComparisonOperation;
import com.regnosys.rosetta.rosetta.expression.ConstructorKeyValuePair;
import com.regnosys.rosetta.rosetta.expression.DefaultOperation;
import com.regnosys.rosetta.rosetta.expression.DistinctOperation;
import com.regnosys.rosetta.rosetta.expression.EqualityOperation;
import com.regnosys.rosetta.rosetta.expression.FilterOperation;
import com.regnosys.rosetta.rosetta.expression.FirstOperation;
import com.regnosys.rosetta.rosetta.expression.FlattenOperation;
import com.regnosys.rosetta.rosetta.expression.InlineFunction;
import com.regnosys.rosetta.rosetta.expression.JoinOperation;
import com.regnosys.rosetta.rosetta.expression.LastOperation;
import com.regnosys.rosetta.rosetta.expression.ListLiteral;
import com.regnosys.rosetta.rosetta.expression.LogicalOperation;
import com.regnosys.rosetta.rosetta.expression.MapOperation;
import com.regnosys.rosetta.rosetta.expression.MaxOperation;
import com.regnosys.rosetta.rosetta.expression.MinOperation;
import com.regnosys.rosetta.rosetta.expression.OneOfOperation;
import com.regnosys.rosetta.rosetta.expression.RosettaOnlyElement;
import com.regnosys.rosetta.rosetta.expression.ReduceOperation;
import com.regnosys.rosetta.rosetta.expression.ReverseOperation;
import com.regnosys.rosetta.rosetta.expression.RosettaAbsentExpression;
import com.regnosys.rosetta.rosetta.expression.RosettaBooleanLiteral;
import com.regnosys.rosetta.rosetta.expression.RosettaConditionalExpression;
import com.regnosys.rosetta.rosetta.expression.RosettaConstructorExpression;
import com.regnosys.rosetta.rosetta.expression.RosettaContainsExpression;
import com.regnosys.rosetta.rosetta.expression.RosettaCountOperation;
import com.regnosys.rosetta.rosetta.expression.RosettaDeepFeatureCall;
import com.regnosys.rosetta.rosetta.expression.RosettaDisjointExpression;
import com.regnosys.rosetta.rosetta.expression.RosettaExistsExpression;
import com.regnosys.rosetta.rosetta.expression.RosettaExpression;
import com.regnosys.rosetta.rosetta.expression.RosettaFeatureCall;
import com.regnosys.rosetta.rosetta.expression.RosettaImplicitVariable;
import com.regnosys.rosetta.rosetta.expression.RosettaIntLiteral;
import com.regnosys.rosetta.rosetta.expression.RosettaNumberLiteral;
import com.regnosys.rosetta.rosetta.expression.RosettaOnlyExistsExpression;
import com.regnosys.rosetta.rosetta.expression.RosettaStringLiteral;
import com.regnosys.rosetta.rosetta.expression.RosettaSymbolReference;
import com.regnosys.rosetta.rosetta.expression.RosettaUnaryOperation;
import com.regnosys.rosetta.rosetta.expression.SortOperation;
import com.regnosys.rosetta.rosetta.expression.SumOperation;
import com.regnosys.rosetta.rosetta.expression.SwitchCaseGuard;
import com.regnosys.rosetta.rosetta.expression.SwitchCaseOrDefault;
import com.regnosys.rosetta.rosetta.expression.SwitchOperation;
import com.regnosys.rosetta.rosetta.expression.ThenOperation;
import com.regnosys.rosetta.rosetta.expression.ToDateOperation;
import com.regnosys.rosetta.rosetta.expression.ToDateTimeOperation;
import com.regnosys.rosetta.rosetta.expression.ToEnumOperation;
import com.regnosys.rosetta.rosetta.expression.ToIntOperation;
import com.regnosys.rosetta.rosetta.expression.ToNumberOperation;
import com.regnosys.rosetta.rosetta.expression.ToTimeOperation;
import com.regnosys.rosetta.rosetta.expression.ToZonedDateTimeOperation;
import com.regnosys.rosetta.rosetta.expression.ToStringOperation;
import com.regnosys.rosetta.rosetta.expression.WithMetaEntry;
import com.regnosys.rosetta.rosetta.expression.WithMetaOperation;
import com.regnosys.rosetta.tests.RosettaTestInjectorProvider;

/**
 * Dumps a normalized JSON view of .rosetta files using the official Java
 * implementation. This is the oracle side of sigil's differential tests; the
 * output mirrors the shape of `sigil model` for the Milestone-1 construct
 * subset.
 */
public class ModelDumper {

    public static void main(String[] args) throws Exception {
        if (args.length == 0) {
            System.err.println("usage: ModelDumper <file.rosetta>...");
            System.exit(2);
        }
        RosettaTestInjectorProvider provider = new RosettaTestInjectorProvider();
        var injector = provider.getInjector();
        ResourceSet resourceSet = injector.getInstance(XtextResourceSet.class);
        RosettaBuiltinsService builtins = injector.getInstance(RosettaBuiltinsService.class);

        // Demand-load the built-in library into the resource set, exactly as
        // ModelHelper.testResourceSet() does in the Java test suite.
        resourceSet.getResource(builtins.basicTypesURI, true);
        resourceSet.getResource(builtins.annotationsURI, true);

        List<Map<String, Object>> fileJsons = new ArrayList<>();
        List<RosettaModel> models = new ArrayList<>();
        for (String path : args) {
            URI uri = URI.createFileURI(new File(path).getAbsolutePath());
            Resource resource = resourceSet.getResource(uri, true);
            if (!resource.getErrors().isEmpty()) {
                System.err.println("parse errors in " + path + ":");
                for (var error : resource.getErrors()) {
                    System.err.println("  " + error);
                }
                System.exit(3);
            }
            EObject root = resource.getContents().get(0);
            if (root instanceof RosettaModel model) {
                models.add(model);
            } else {
                System.err.println("not a RosettaModel: " + path);
                System.exit(2);
            }
        }
        EcoreUtil2.resolveAll(resourceSet);
        for (RosettaModel model : models) {
            fileJsons.add(modelJson(model));
        }

        Map<String, Object> out = new LinkedHashMap<>();
        out.put("format", "sigil-model/1 (oracle)");
        out.put("files", fileJsons);
        System.out.print(new ObjectMapper().writeValueAsString(out));
    }

    private static Map<String, Object> modelJson(RosettaModel model) {
        Map<String, Object> json = new LinkedHashMap<>();
        json.put("name", model.eResource() == null ? null
                : new File(model.eResource().getURI().toFileString()).getName());
        json.put("namespace", model.getName());
        json.put("overridden", model.isOverridden());
        json.put("scope", null);
        json.put("version", model.getVersion());
        List<Object> imports = new ArrayList<>();
        for (Import imp : model.getImports()) {
            Map<String, Object> jsonImport = new LinkedHashMap<>();
            jsonImport.put("importedNamespace", imp.getImportedNamespace());
            jsonImport.put("wildcard", imp.getImportedNamespace().endsWith(".*"));
            jsonImport.put("namespaceAlias", imp.getNamespaceAlias());
            imports.add(jsonImport);
        }
        json.put("imports", imports);

        List<Object> configurations = new ArrayList<>();
        for (RosettaQualifiableConfiguration config : model.getConfigurations()) {
            Map<String, Object> jsonConfig = new LinkedHashMap<>();
            jsonConfig.put("qType", config.getQType().getName());
            jsonConfig.put("root", typeRefJson(config.getRosettaClass()));
            configurations.add(jsonConfig);
        }
        json.put("configurations", configurations);

        List<Object> elements = new ArrayList<>();
        for (EObject element : model.getElements()) {
            Map<String, Object> jsonElement = elementJson(element);
            if (jsonElement != null) {
                elements.add(jsonElement);
            }
        }
        json.put("elements", elements);
        return json;
    }

    private static Map<String, Object> elementJson(EObject element) {
        if (element instanceof Data data) {
            Map<String, Object> json = new LinkedHashMap<>();
            json.put("kind", element instanceof Choice ? "Choice" : "Data");
            json.put("name", data.getName());
            json.put("definition", data.getDefinition());
            json.put("superType", data.getSuperType() == null ? null
                    : fqn(data.getSuperType()));
            json.put("annotations", annotationsJson(data.getAnnotations()));
            json.put("docReferences", docRefsJson(data.getReferences()));
            json.put("attributes", attributesJson(data.getAttributes()));
            List<Object> conditions = new ArrayList<>();
            for (Condition condition : data.getConditions()) {
                Map<String, Object> jsonCondition = new LinkedHashMap<>();
                jsonCondition.put("name", condition.getName());
                jsonCondition.put("definition", condition.getDefinition());
                jsonCondition.put("annotations", annotationsJson(condition.getAnnotations()));
                jsonCondition.put("docReferences", docRefsJson(condition.getReferences()));
                jsonCondition.put("expression", expressionJson(condition.getExpression()));
                conditions.add(jsonCondition);
            }
            json.put("conditions", conditions);
            return json;
        }
        if (element instanceof RosettaEnumeration enumeration) {
            Map<String, Object> json = new LinkedHashMap<>();
            json.put("kind", "Enumeration");
            json.put("name", enumeration.getName());
            json.put("definition", enumeration.getDefinition());
            json.put("superType", enumeration.getParent() == null ? null
                    : fqn(enumeration.getParent()));
            json.put("annotations", annotationsJson(enumeration.getAnnotations()));
            json.put("docReferences", docRefsJson(enumeration.getReferences()));
            List<Object> values = new ArrayList<>();
            for (RosettaEnumValue value : enumeration.getEnumValues()) {
                Map<String, Object> jsonValue = new LinkedHashMap<>();
                jsonValue.put("name", value.getName());
                jsonValue.put("display", value.getDisplay());
                jsonValue.put("definition", value.getDefinition());
                jsonValue.put("annotations", annotationsJson(value.getAnnotations()));
                jsonValue.put("docReferences", docRefsJson(value.getReferences()));
                values.add(jsonValue);
            }
            json.put("values", values);
            return json;
        }
        if (element instanceof Annotation annotation) {
            Map<String, Object> json = new LinkedHashMap<>();
            json.put("kind", "Annotation");
            json.put("name", annotation.getName());
            json.put("definition", annotation.getDefinition());
            json.put("prefix", annotation.getPrefix());
            json.put("attributes", attributesJson(annotation.getAttributes()));
            return json;
        }
        if (element instanceof RosettaTypeAlias alias) {
            Map<String, Object> json = new LinkedHashMap<>();
            json.put("kind", "TypeAlias");
            json.put("name", alias.getName());
            json.put("definition", alias.getDefinition());
            json.put("parameters", parametersJson(alias.getParameters()));
            json.put("type", typeCallJson(alias.getTypeCall()));
            // NOTE: the published oracle (9.58.1) predates annotations on
            // type aliases; the compare script normalizes this field away.
            return json;
        }
        if (element instanceof RosettaBasicType basic) {
            Map<String, Object> json = new LinkedHashMap<>();
            json.put("kind", "BasicType");
            json.put("name", basic.getName());
            json.put("definition", basic.getDefinition());
            json.put("parameters", parametersJson(basic.getParameters()));
            return json;
        }
        if (element instanceof RosettaRecordType record) {
            Map<String, Object> json = new LinkedHashMap<>();
            json.put("kind", "RecordType");
            json.put("name", record.getName());
            json.put("definition", record.getDefinition());
            List<Object> features = new ArrayList<>();
            for (RosettaRecordFeature feature : record.getFeatures()) {
                Map<String, Object> jsonFeature = new LinkedHashMap<>();
                jsonFeature.put("name", feature.getName());
                jsonFeature.put("type", typeCallJson(feature.getTypeCall()));
                features.add(jsonFeature);
            }
            json.put("features", features);
            return json;
        }
        if (element instanceof RosettaExternalFunction function) {
            Map<String, Object> json = new LinkedHashMap<>();
            json.put("kind", "LibraryFunction");
            json.put("name", function.getName());
            json.put("definition", function.getDefinition());
            List<Object> parameters = new ArrayList<>();
            for (RosettaParameter parameter : function.getParameters()) {
                Map<String, Object> jsonParameter = new LinkedHashMap<>();
                jsonParameter.put("name", parameter.getName());
                jsonParameter.put("type", typeCallJson(parameter.getTypeCall()));
                jsonParameter.put("isArray", parameter.isIsArray());
                parameters.add(jsonParameter);
            }
            json.put("parameters", parameters);
            json.put("returnType", typeCallJson(function.getTypeCall()));
            return json;
        }
        if (element instanceof Function function) {
            Map<String, Object> json = new LinkedHashMap<>();
            json.put("kind", "Function");
            json.put("name", function.getName());
            json.put("definition", function.getDefinition());
            if (function instanceof FunctionDispatch dispatch) {
                Map<String, Object> jsonDispatch = new LinkedHashMap<>();
                jsonDispatch.put("attribute", dispatch.getAttribute() == null ? null
                        : dispatch.getAttribute().getName());
                jsonDispatch.put("enumeration", dispatch.getValue() == null
                        || dispatch.getValue().getEnumeration() == null ? null
                        : dispatch.getValue().getEnumeration().getName());
                jsonDispatch.put("value", dispatch.getValue() == null
                        || dispatch.getValue().getValue() == null ? null
                        : dispatch.getValue().getValue().getName());
                json.put("dispatch", jsonDispatch);
            } else {
                json.put("dispatch", null);
            }
            json.put("annotations", annotationsJson(function.getAnnotations()));
            json.put("docReferences", docRefsJson(function.getReferences()));
            json.put("inputs", attributesJson(function.getInputs()));
            json.put("output", function.getOutput() == null ? null
                    : attributesJson(List.of(function.getOutput())).get(0));
            List<Object> shortcuts = new ArrayList<>();
            for (ShortcutDeclaration shortcut : function.getShortcuts()) {
                Map<String, Object> jsonShortcut = new LinkedHashMap<>();
                jsonShortcut.put("name", shortcut.getName());
                jsonShortcut.put("definition", shortcut.getDefinition());
                jsonShortcut.put("expression", expressionJson(shortcut.getExpression()));
                shortcuts.add(jsonShortcut);
            }
            json.put("shortcuts", shortcuts);
            json.put("conditions", conditionsJson(function.getConditions()));
            List<Object> operations = new ArrayList<>();
            for (Operation op : function.getOperations()) {
                Map<String, Object> jsonOperation = new LinkedHashMap<>();
                jsonOperation.put("definition", op.getDefinition());
                jsonOperation.put("add", op.isAdd());
                jsonOperation.put("assignRoot", op.getAssignRoot() == null ? null
                        : op.getAssignRoot().getName());
                List<Object> path = new ArrayList<>();
                Segment segment = op.getPath();
                while (segment != null) {
                    path.add(segment.getFeature() == null ? null : segment.getFeature().getName());
                    segment = segment.getNext();
                }
                jsonOperation.put("path", path);
                jsonOperation.put("expression", expressionJson(op.getExpression()));
                operations.add(jsonOperation);
            }
            json.put("operations", operations);
            json.put("postConditions", conditionsJson(function.getPostConditions()));
            return json;
        }
        if (element instanceof RosettaRule rule) {
            Map<String, Object> json = new LinkedHashMap<>();
            json.put("kind", "Rule");
            json.put("name", rule.getName());
            json.put("definition", rule.getDefinition());
            json.put("eligibility", rule.isEligibility());
            json.put("input", typeCallJson(rule.getInput()));
            json.put("expression", expressionJson(rule.getExpression()));
            return json;
        }
        if (element instanceof RosettaReport report) {
            Map<String, Object> json = new LinkedHashMap<>();
            json.put("kind", "Report");
            Map<String, Object> body = new LinkedHashMap<>();
            var regulatory = report.getRegulatoryBody();
            body.put("body", regulatory == null || regulatory.getBody() == null ? null
                    : fqn(regulatory.getBody()));
            List<Object> corpora = new ArrayList<>();
            if (regulatory != null) {
                for (var corpus : regulatory.getCorpusList()) {
                    corpora.add(fqn(corpus));
                }
            }
            body.put("corpora", corpora);
            List<Object> segments = new ArrayList<>();
            if (regulatory != null) {
                for (var segment : regulatory.getSegments()) {
                    Map<String, Object> jsonSegment = new LinkedHashMap<>();
                    jsonSegment.put("segment", segment.getSegment() == null ? null
                            : fqn(segment.getSegment()));
                    jsonSegment.put("reference", segment.getSegmentRef());
                    segments.add(jsonSegment);
                }
            }
            body.put("segments", segments);
            json.put("regulatoryBody", body);
            List<Object> eligibilityRules = new ArrayList<>();
            for (var rule : report.getEligibilityRules()) {
                eligibilityRules.add(fqn(rule));
            }
            json.put("eligibilityRules", eligibilityRules);
            json.put("inputType", typeCallJson(report.getInputType()));
            json.put("reportType", report.getReportType() == null ? null
                    : fqn(report.getReportType()));
            json.put("ruleSource", report.getRuleSource() == null ? null
                    : fqn(report.getRuleSource()));
            return json;
        }
        if (element instanceof RosettaExternalRuleSource source) {
            Map<String, Object> json = new LinkedHashMap<>();
            json.put("kind", "ExternalRuleSource");
            json.put("name", source.getName());
            json.put("superSource", source.getSuperRuleSources().isEmpty() ? null
                    : fqn(source.getSuperRuleSources().get(0)));
            List<Object> classes = new ArrayList<>();
            for (var externalClass : source.getExternalClasses()) {
                Map<String, Object> jsonClass = new LinkedHashMap<>();
                jsonClass.put("data", externalClass.getData() == null ? null
                        : fqn(externalClass.getData()));
                List<Object> attributes = new ArrayList<>();
                for (var attribute : externalClass.getRegularAttributes()) {
                    Map<String, Object> jsonAttribute = new LinkedHashMap<>();
                    jsonAttribute.put("operator",
                            attribute.getOperator() == com.regnosys.rosetta.rosetta.ExternalValueOperator.PLUS
                                    ? "+"
                                    : "-");
                    jsonAttribute.put("attribute", attribute.getAttributeRef() == null ? null
                            : attribute.getAttributeRef().getName());
                    List<Object> ruleRefs = new ArrayList<>();
                    for (RuleReferenceAnnotation rule : attribute.getExternalRuleReferences()) {
                        Map<String, Object> jsonRule = new LinkedHashMap<>();
                        jsonRule.put("rule", rule.getReportingRule() == null ? null
                                : fqn(rule.getReportingRule()));
                        jsonRule.put("empty", rule.isEmpty());
                        ruleRefs.add(jsonRule);
                    }
                    jsonAttribute.put("ruleReferences", ruleRefs);
                    attributes.add(jsonAttribute);
                }
                jsonClass.put("attributes", attributes);
                classes.add(jsonClass);
            }
            json.put("externalClasses", classes);
            return json;
        }
        if (element instanceof RosettaBody body) {
            Map<String, Object> json = new LinkedHashMap<>();
            json.put("kind", "Body");
            json.put("name", body.getName());
            json.put("bodyType", body.getBodyType());
            json.put("definition", body.getDefinition());
            return json;
        }
        if (element instanceof RosettaCorpus corpus) {
            Map<String, Object> json = new LinkedHashMap<>();
            json.put("kind", "Corpus");
            json.put("name", corpus.getName());
            json.put("corpusType", corpus.getCorpusType());
            json.put("displayName", corpus.getDisplayName());
            json.put("body", corpus.getBody() == null ? null : corpus.getBody().getName());
            json.put("definition", corpus.getDefinition());
            return json;
        }
        if (element instanceof RosettaSegment segment) {
            Map<String, Object> json = new LinkedHashMap<>();
            json.put("kind", "Segment");
            json.put("name", segment.getName());
            return json;
        }
        if (element instanceof RosettaMetaType metaType) {
            Map<String, Object> json = new LinkedHashMap<>();
            json.put("kind", "MetaType");
            json.put("name", metaType.getName());
            json.put("type", typeCallJson(metaType.getTypeCall()));
            return json;
        }
        return null; // construct outside the M1 subset
    }

    private static List<Object> conditionsJson(EList<Condition> conditions) {
        List<Object> out = new ArrayList<>();
        for (Condition condition : conditions) {
            Map<String, Object> jsonCondition = new LinkedHashMap<>();
            jsonCondition.put("name", condition.getName());
            jsonCondition.put("definition", condition.getDefinition());
            jsonCondition.put("annotations", annotationsJson(condition.getAnnotations()));
            jsonCondition.put("docReferences", docRefsJson(condition.getReferences()));
            jsonCondition.put("expression", expressionJson(condition.getExpression()));
            out.add(jsonCondition);
        }
        return out;
    }

    private static List<Object> attributesJson(List<Attribute> attributes) {
        List<Object> jsonAttributes = new ArrayList<>();
        for (Attribute attribute : attributes) {
            Map<String, Object> jsonAttribute = new LinkedHashMap<>();
            jsonAttribute.put("name", attribute.getName());
            jsonAttribute.put("override", attribute.isOverride());
            jsonAttribute.put("type", typeCallJson(attribute.getTypeCall()));
            RosettaCardinality card = attribute.getCard();
            jsonAttribute.put("cardinality", card == null ? null : card.toConstraintString());
            jsonAttribute.put("definition", attribute.getDefinition());
            jsonAttribute.put("annotations", annotationsJson(attribute.getAnnotations()));
            List<Object> labels = new ArrayList<>();
            for (LabelAnnotation label : attribute.getLabels()) {
                Map<String, Object> jsonLabel = new LinkedHashMap<>();
                jsonLabel.put("label", label.getLabel());
                labels.add(jsonLabel);
            }
            jsonAttribute.put("labels", labels);
            List<Object> ruleReferences = new ArrayList<>();
            for (RuleReferenceAnnotation rule : attribute.getRuleReferences()) {
                Map<String, Object> jsonRule = new LinkedHashMap<>();
                jsonRule.put("rule", rule.getReportingRule() == null ? null
                        : fqn(rule.getReportingRule()));
                jsonRule.put("empty", rule.isEmpty());
                ruleReferences.add(jsonRule);
            }
            jsonAttribute.put("ruleReferences", ruleReferences);
            jsonAttribute.put("docReferences", docRefsJson(attribute.getReferences()));
            jsonAttributes.add(jsonAttribute);
        }
        return jsonAttributes;
    }

    private static List<Object> parametersJson(EList<TypeParameter> parameters) {
        List<Object> jsonParameters = new ArrayList<>();
        for (TypeParameter parameter : parameters) {
            Map<String, Object> jsonParameter = new LinkedHashMap<>();
            jsonParameter.put("name", parameter.getName());
            jsonParameter.put("type", typeCallJson(parameter.getTypeCall()));
            jsonParameter.put("definition", parameter.getDefinition());
            jsonParameters.add(jsonParameter);
        }
        return jsonParameters;
    }

    private static List<Object> annotationsJson(EList<AnnotationRef> annotations) {
        List<Object> jsonAnnotations = new ArrayList<>();
        for (AnnotationRef ref : annotations) {
            Map<String, Object> jsonAnnotation = new LinkedHashMap<>();
            Map<String, Object> annotation = new LinkedHashMap<>();
            annotation.put("name", ref.getAnnotation() == null ? null
                    : ref.getAnnotation().getName());
            annotation.put("resolved", ref.getAnnotation() == null ? null
                    : fqn(ref.getAnnotation()));
            jsonAnnotation.put("annotation", annotation);
            jsonAnnotation.put("attribute",
                    ref.getAttribute() == null ? null : ref.getAttribute().getName());
            List<Object> qualifiers = new ArrayList<>();
            for (AnnotationQualifier qualifier : ref.getQualifiers()) {
                Map<String, Object> jsonQualifier = new LinkedHashMap<>();
                jsonQualifier.put("name", qualifier.getQualName());
                // String values are compared fully; qualifier paths are
                // reduced to a marker (the oracle's path model differs from
                // sigil's and is outside the M1 comparison scope).
                jsonQualifier.put("value", qualifier.getQualValue() != null
                        ? Map.of("kind", "Str", "value", qualifier.getQualValue())
                        : Map.of("kind", "Path"));
                qualifiers.add(jsonQualifier);
            }
            jsonAnnotation.put("qualifiers", qualifiers);
            jsonAnnotations.add(jsonAnnotation);
        }
        return jsonAnnotations;
    }

    private static List<Object> docRefsJson(EList<RosettaDocReference> docRefs) {
        List<Object> jsonDocRefs = new ArrayList<>();
        for (RosettaDocReference doc : docRefs) {
            Map<String, Object> jsonDoc = new LinkedHashMap<>();
            jsonDoc.put("body", doc.getDocReference() == null
                    || doc.getDocReference().getBody() == null ? null
                    : doc.getDocReference().getBody().getName());
            List<Object> corpora = new ArrayList<>();
            if (doc.getDocReference() != null) {
                for (var corpus : doc.getDocReference().getCorpusList()) {
                    corpora.add(corpus.getName());
                }
            }
            jsonDoc.put("corpora", corpora);
            jsonDoc.put("reportedField", doc.isReportedField());
            jsonDocRefs.add(jsonDoc);
        }
        return jsonDocRefs;
    }

    private static Map<String, Object> typeCallJson(TypeCall typeCall) {
        if (typeCall == null) {
            return null;
        }
        Map<String, Object> json = new LinkedHashMap<>();
        json.put("name", sourceText(typeCall, "type"));
        List<Object> arguments = new ArrayList<>();
        for (TypeCallArgument argument : typeCall.getArguments()) {
            Map<String, Object> jsonArgument = new LinkedHashMap<>();
            jsonArgument.put("parameter",
                    argument.getParameter() == null ? null : argument.getParameter().getName());
            jsonArgument.put("value", argument.getValue() == null ? null
                    : org.eclipse.xtext.nodemodel.util.NodeModelUtils.getTokenText(
                            org.eclipse.xtext.nodemodel.util.NodeModelUtils
                                    .findActualNodeFor(argument.getValue())));
            arguments.add(jsonArgument);
        }
        json.put("arguments", arguments);
        json.put("resolved", typeCall.getType() == null ? null : fqn(typeCall.getType()));
        return json;
    }

    private static Map<String, Object> typeRefJson(EObject type) {
        Map<String, Object> json = new LinkedHashMap<>();
        json.put("name", null);
        json.put("arguments", new ArrayList<>());
        json.put("resolved", type == null ? null : fqn(type));
        return json;
    }

    private static String sourceText(EObject object, String featureName) {
        var feature = object.eClass().getEStructuralFeature(featureName);
        if (feature == null) {
            return null;
        }
        var nodes = org.eclipse.xtext.nodemodel.util.NodeModelUtils
                .findNodesForFeature(object, feature);
        if (nodes.isEmpty()) {
            return null;
        }
        StringBuilder b = new StringBuilder();
        for (var node : nodes) {
            b.append(node.getText().trim());
        }
        return b.toString();
    }

    // ---- expressions ---------------------------------------------------------
    // Normalized expression JSON. The tag names mirror the ones sigil emits;
    // the mapping from EClass names is documented in
    // scripts/oracle_compare.py.

    private static Map<String, Object> expressionJson(RosettaExpression expr) {
        if (expr == null) {
            return null;
        }
        Map<String, Object> json = new LinkedHashMap<>();
        String cls = expr.eClass().getName();
        switch (cls) {
            case "RosettaBooleanLiteral" -> {
                json.put("kind", "Boolean");
                json.put("value", ((RosettaBooleanLiteral) expr).isValue());
            }
            case "RosettaStringLiteral" -> {
                json.put("kind", "String");
                json.put("value", ((RosettaStringLiteral) expr).getValue());
            }
            case "RosettaNumberLiteral" -> {
                json.put("kind", "Number");
                json.put("text", nodeText(expr));
            }
            case "RosettaIntLiteral" -> {
                json.put("kind", "Int");
                json.put("text", nodeText(expr));
            }
            case "ListLiteral" -> {
                json.put("kind", "List");
                json.put("elements", expressionsJson(((ListLiteral) expr).getElements()));
            }
            case "RosettaSymbolReference" -> {
                json.put("kind", "SymbolReference");
                json.put("symbol", refText(expr, "symbol"));
                json.put("explicit", ((RosettaSymbolReference) expr).isExplicitArguments());
                json.put("args", expressionsJson(((RosettaSymbolReference) expr).getRawArgs()));
            }
            case "RosettaImplicitVariable" -> json.put("kind", "ImplicitVariable");
            case "RosettaFeatureCall" -> {
                json.put("kind", "FeatureCall");
                json.put("receiver", expressionJson(((RosettaFeatureCall) expr).getReceiver()));
                json.put("feature", refText(expr, "feature"));
            }
            case "RosettaDeepFeatureCall" -> {
                json.put("kind", "DeepFeatureCall");
                json.put("receiver", expressionJson(((RosettaDeepFeatureCall) expr).getReceiver()));
                json.put("feature", refText(expr, "feature"));
            }
            case "ArithmeticOperation", "LogicalOperation", "RosettaContainsExpression",
                 "RosettaDisjointExpression", "DefaultOperation" -> {
                json.put("kind", "Binary");
                json.put("op", expr.eGet(expr.eClass().getEStructuralFeature("operator")));
                json.put("left", expressionJson(binaryLeft(expr)));
                json.put("right", expressionJson(binaryRight(expr)));
            }
            case "EqualityOperation" -> {
                json.put("kind", "Binary");
                json.put("op", ((EqualityOperation) expr).getOperator());
                json.put("cardMod", ((EqualityOperation) expr).getCardMod().getLiteral());
                json.put("left", expressionJson(binaryLeft(expr)));
                json.put("right", expressionJson(binaryRight(expr)));
            }
            case "ComparisonOperation" -> {
                json.put("kind", "Binary");
                json.put("op", ((ComparisonOperation) expr).getOperator());
                json.put("cardMod", ((ComparisonOperation) expr).getCardMod().getLiteral());
                json.put("left", expressionJson(binaryLeft(expr)));
                json.put("right", expressionJson(binaryRight(expr)));
            }
            case "JoinOperation" -> {
                json.put("kind", "Join");
                json.put("left", expressionJson(((JoinOperation) expr).getLeft()));
                json.put("right", expressionJson(((JoinOperation) expr).getRight()));
                json.put("explicitSeparator", ((JoinOperation) expr).isExplicitSeparator());
            }
            case "RosettaConditionalExpression" -> {
                RosettaConditionalExpression cond = (RosettaConditionalExpression) expr;
                json.put("kind", "Conditional");
                json.put("if", expressionJson(cond.getIf()));
                json.put("then", expressionJson(cond.getIfthen()));
                json.put("else", expressionJson(cond.getElsethen()));
                json.put("full", cond.isFull());
            }
            case "RosettaOnlyExistsExpression" -> {
                RosettaOnlyExistsExpression oe = (RosettaOnlyExistsExpression) expr;
                json.put("kind", "OnlyExists");
                json.put("args", expressionsJson(oe.getArgs()));
                json.put("parentheses", oe.isHasParentheses());
            }
            case "RosettaExistsExpression" -> {
                RosettaExistsExpression ex = (RosettaExistsExpression) expr;
                json.put("kind", "Exists");
                json.put("modifier", ex.getModifier().getLiteral().toLowerCase());
                json.put("argument", expressionJson(ex.getArgument()));
            }
            case "RosettaAbsentExpression" -> unaryJson(json, "Absent",
                    ((RosettaAbsentExpression) expr).getArgument());
            case "RosettaOnlyElement" -> unaryJson(json, "OnlyElement",
                    ((RosettaOnlyElement) expr).getArgument());
            case "RosettaCountOperation" -> unaryJson(json, "Count",
                    ((RosettaCountOperation) expr).getArgument());
            case "FlattenOperation" -> unaryJson(json, "Flatten",
                    ((FlattenOperation) expr).getArgument());
            case "DistinctOperation" -> unaryJson(json, "Distinct",
                    ((DistinctOperation) expr).getArgument());
            case "ReverseOperation" -> unaryJson(json, "Reverse",
                    ((ReverseOperation) expr).getArgument());
            case "FirstOperation" -> unaryJson(json, "First",
                    ((FirstOperation) expr).getArgument());
            case "LastOperation" -> unaryJson(json, "Last",
                    ((LastOperation) expr).getArgument());
            case "SumOperation" -> unaryJson(json, "Sum",
                    ((SumOperation) expr).getArgument());
            case "AsKeyOperation" -> unaryJson(json, "AsKey",
                    ((AsKeyOperation) expr).getArgument());
            case "OneOfOperation" -> unaryJson(json, "OneOf",
                    ((OneOfOperation) expr).getArgument());
            case "ToStringOperation" -> unaryJson(json, "ToString",
                    ((ToStringOperation) expr).getArgument());
            case "ToNumberOperation" -> unaryJson(json, "ToNumber",
                    ((ToNumberOperation) expr).getArgument());
            case "ToIntOperation" -> unaryJson(json, "ToInt",
                    ((ToIntOperation) expr).getArgument());
            case "ToTimeOperation" -> unaryJson(json, "ToTime",
                    ((ToTimeOperation) expr).getArgument());
            case "ToDateOperation" -> unaryJson(json, "ToDate",
                    ((ToDateOperation) expr).getArgument());
            case "ToDateTimeOperation" -> unaryJson(json, "ToDateTime",
                    ((ToDateTimeOperation) expr).getArgument());
            case "ToZonedDateTimeOperation" -> unaryJson(json, "ToZonedDateTime",
                    ((ToZonedDateTimeOperation) expr).getArgument());
            case "ToEnumOperation" -> {
                json.put("kind", "ToEnum");
                json.put("enumeration", refText(expr, "enumeration"));
                json.put("argument", expressionJson(((ToEnumOperation) expr).getArgument()));
            }
            case "ChoiceOperation" -> {
                ChoiceOperation choice = (ChoiceOperation) expr;
                json.put("kind", "Choice");
                json.put("necessity", choice.getNecessity().getLiteral());
                List<Object> attributes = new ArrayList<>();
                for (var attribute : choice.getAttributes()) {
                    attributes.add(attribute.getName());
                }
                json.put("attributes", attributes);
                json.put("argument", expressionJson(choice.getArgument()));
            }
            case "SwitchOperation" -> {
                SwitchOperation sw = (SwitchOperation) expr;
                json.put("kind", "Switch");
                json.put("argument", expressionJson(sw.getArgument()));
                List<Object> cases = new ArrayList<>();
                for (SwitchCaseOrDefault c : sw.getCases()) {
                    Map<String, Object> jsonCase = new LinkedHashMap<>();
                    if (c.isDefault()) {
                        jsonCase.put("default", true);
                    } else {
                        SwitchCaseGuard guard = c.getGuard();
                        if (guard.getLiteralGuard() != null) {
                            Map<String, Object> jsonGuard = new LinkedHashMap<>();
                            jsonGuard.put("kind", "Literal");
                            jsonGuard.put("value", expressionJson(guard.getLiteralGuard()));
                            jsonCase.put("guard", jsonGuard);
                        } else {
                            Map<String, Object> jsonGuard = new LinkedHashMap<>();
                            jsonGuard.put("kind", "Reference");
                            jsonGuard.put("target", refText(guard, "symbolGuard"));
                            jsonCase.put("guard", jsonGuard);
                        }
                    }
                    jsonCase.put("expression", expressionJson(c.getExpression()));
                    cases.add(jsonCase);
                }
                json.put("cases", cases);
            }
            case "WithMetaOperation" -> {
                WithMetaOperation wm = (WithMetaOperation) expr;
                json.put("kind", "WithMeta");
                json.put("argument", expressionJson(wm.getArgument()));
                List<Object> entries = new ArrayList<>();
                for (WithMetaEntry entry : wm.getEntries()) {
                    Map<String, Object> jsonEntry = new LinkedHashMap<>();
                    jsonEntry.put("key", entry.getKey() == null ? null : entry.getKey().getName());
                    jsonEntry.put("value", expressionJson(entry.getValue()));
                    entries.add(jsonEntry);
                }
                json.put("entries", entries);
            }
            // Note: AsOperation/as-key do not exist in 9.58.1 (newer-main
            // syntax); fixtures avoid them and sigil-only tests cover them.
            case "ThenOperation", "FilterOperation", "MapOperation", "ReduceOperation",
                 "SortOperation", "MinOperation", "MaxOperation" -> {
                json.put("kind", cls.equals("ThenOperation") ? "Then"
                        : cls.equals("FilterOperation") ? "Filter"
                        : cls.equals("MapOperation") ? "Map"
                        : cls.equals("ReduceOperation") ? "Reduce"
                        : cls.equals("SortOperation") ? "Sort"
                        : cls.equals("MinOperation") ? "Min" : "Max");
                RosettaUnaryOperation functional = (RosettaUnaryOperation) expr;
                json.put("argument", expressionJson(functional.getArgument()));
                json.put("function", inlineFunctionJson(
                        (InlineFunction) expr.eGet(expr.eClass()
                                .getEStructuralFeature("function"))));
            }
            case "RosettaConstructorExpression" -> {
                RosettaConstructorExpression ctor = (RosettaConstructorExpression) expr;
                Map<String, Object> type = new LinkedHashMap<>();
                type.put("name", ctor.getTypeCall() == null ? null
                        : sourceText(ctor.getTypeCall(), "type"));
                List<Object> arguments = new ArrayList<>();
                if (ctor.getTypeCall() != null) {
                    for (TypeCallArgument argument : ctor.getTypeCall().getArguments()) {
                        Map<String, Object> jsonArgument = new LinkedHashMap<>();
                        jsonArgument.put("parameter",
                                argument.getParameter() == null ? null : argument.getParameter().getName());
                        jsonArgument.put("value", argument.getValue() == null ? null
                                : org.eclipse.xtext.nodemodel.util.NodeModelUtils.getTokenText(
                                        org.eclipse.xtext.nodemodel.util.NodeModelUtils
                                                .findActualNodeFor(argument.getValue())));
                        arguments.add(jsonArgument);
                    }
                }
                type.put("arguments", arguments);
                json.put("kind", "Constructor");
                json.put("type", type);
                List<Object> values = new ArrayList<>();
                for (ConstructorKeyValuePair pair : ctor.getValues()) {
                    Map<String, Object> jsonPair = new LinkedHashMap<>();
                    jsonPair.put("key", pair.getKey() == null ? null : pair.getKey().getName());
                    jsonPair.put("value", expressionJson(pair.getValue()));
                    values.add(jsonPair);
                }
                json.put("values", values);
                json.put("implicitEmpty", ctor.isImplicitEmpty());
            }
            default -> throw new IllegalStateException("unhandled expression class: " + cls);
        }
        return json;
    }

    private static Map<String, Object> inlineFunctionJson(InlineFunction function) {
        if (function == null) {
            return null;
        }
        Map<String, Object> json = new LinkedHashMap<>();
        List<Object> parameters = new ArrayList<>();
        for (var parameter : function.getParameters()) {
            parameters.add(parameter.getName());
        }
        json.put("parameters", parameters);
        json.put("body", expressionJson(function.getBody()));
        return json;
    }

    private static void unaryJson(Map<String, Object> json, String kind, RosettaExpression argument) {
        json.put("kind", kind);
        json.put("argument", expressionJson(argument));
    }

    private static RosettaExpression binaryLeft(EObject expr) {
        return (RosettaExpression) expr.eGet(expr.eClass().getEStructuralFeature("left"));
    }

    private static RosettaExpression binaryRight(EObject expr) {
        return (RosettaExpression) expr.eGet(expr.eClass().getEStructuralFeature("right"));
    }

    private static List<Object> expressionsJson(List<RosettaExpression> expressions) {
        List<Object> out = new ArrayList<>();
        for (RosettaExpression expression : expressions) {
            out.add(expressionJson(expression));
        }
        return out;
    }

    /** The written source text of a cross-reference feature. */
    private static String refText(EObject object, String featureName) {
        var feature = object.eClass().getEStructuralFeature(featureName);
        if (feature == null) {
            return null;
        }
        StringBuilder b = new StringBuilder();
        for (var node : org.eclipse.xtext.nodemodel.util.NodeModelUtils
                .findNodesForFeature(object, feature)) {
            b.append(node.getText().trim());
        }
        String text = b.toString();
        return text.isEmpty() ? null : text;
    }

    /** The exact source text of a literal node (preserves `1.50`, signs...). */
    private static String nodeText(EObject object) {
        var node = org.eclipse.xtext.nodemodel.util.NodeModelUtils.findActualNodeFor(object);
        return node == null ? null : node.getText().trim();
    }

    private static String fqn(EObject type) {
        if (type == null || type.eIsProxy()) {
            return null;
        }
        EObject container = type.eContainer();
        while (container != null && !(container instanceof RosettaModel)) {
            container = container.eContainer();
        }
        String name = named(type);
        if (container == null) {
            return name;
        }
        return ((RosettaModel) container).getName() + "." + name;
    }

    private static String named(EObject object) {
        try {
            var feature = object.eClass().getEStructuralFeature("name");
            return feature == null ? null : String.valueOf(object.eGet(feature));
        } catch (Exception e) {
            return null;
        }
    }
}
