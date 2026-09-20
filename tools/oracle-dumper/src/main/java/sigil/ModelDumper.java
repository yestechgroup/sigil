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
import com.regnosys.rosetta.rosetta.RosettaCardinality;
import com.regnosys.rosetta.rosetta.RosettaDocReference;
import com.regnosys.rosetta.rosetta.RosettaEnumValue;
import com.regnosys.rosetta.rosetta.RosettaEnumeration;
import com.regnosys.rosetta.rosetta.RosettaExternalFunction;
import com.regnosys.rosetta.rosetta.RosettaModel;
import com.regnosys.rosetta.rosetta.RosettaParameter;
import com.regnosys.rosetta.rosetta.RosettaQualifiableConfiguration;
import com.regnosys.rosetta.rosetta.RosettaRecordFeature;
import com.regnosys.rosetta.rosetta.RosettaRecordType;
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
        return null; // construct outside the M1 subset
    }

    private static List<Object> attributesJson(EList<Attribute> attributes) {
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
