package net.litematicaba.mc26oracle;

import java.io.IOException;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;
import javax.tools.JavaCompiler;
import javax.tools.ToolProvider;

/** The old liquid/collision APIs require World rather than IBlockAccess. This
 * tiny target-typed subclass forwards only the same empty-world data queries.
 * It is not a client/server simulation; no vanilla World constructor runs. */
public final class LegacyWorldAccess {
    private static final Map<Object, Binding> BINDINGS = new IdentityHashMap<Object, Binding>();
    private static final class Binding {
        final InvocationHandler handler; final List<Method> queries;
        Binding(InvocationHandler handler,List<Method>queries){this.handler=handler;this.queries=queries;}
    }
    private LegacyWorldAccess() {}

    public static Object query(Object world,int index,Object[]arguments)throws Throwable{
        Binding binding=BINDINGS.get(world);
        if(binding==null)throw new IllegalStateException("unbound legacy empty World");
        return binding.handler.invoke(world,binding.queries.get(index),arguments);
    }

    public static Object create(MappingAccess access,InvocationHandler handler,Path generated)throws Exception{
        Class<?> world=access.type("net.minecraft.world.World"),blockAccess=access.type("net.minecraft.world.IBlockAccess");
        if(world.getPackage()!=null&&!world.getPackage().getName().isEmpty())throw new IllegalStateException("expected exact obfuscated legacy client in default package");
        List<Method>queries=new ArrayList<Method>();
        Map<String,Method> declarations=new HashMap<String,Method>();
        for(Class<?>type=world;type!=null;type=type.getSuperclass())for(Method method:type.getDeclaredMethods()){
            String key=signature(method);if(!declarations.containsKey(key))declarations.put(key,method);
        }
        StringBuilder source=new StringBuilder("public final class NovaLegacyEmptyWorld extends ").append(world.getName()).append(" {\n");
        Constructor<?> constructor=null;
        for(Constructor<?> candidate:world.getDeclaredConstructors())if(!Modifier.isPrivate(candidate.getModifiers())){constructor=candidate;break;}
        if(constructor==null)throw new IllegalStateException("legacy World has no callable super constructor");
        source.append("public NovaLegacyEmptyWorld(");parameters(source,constructor.getParameterTypes());source.append(") throws Exception { super(");
        for(int i=0;i<constructor.getParameterTypes().length;i++){if(i>0)source.append(',');source.append('p').append(i);}source.append("); }\n");
        for(Method query:blockAccess.getMethods()){
            Method implementation=declarations.get(signature(query));
            if(implementation!=null&&Modifier.isFinal(implementation.getModifiers()))throw new IllegalStateException("cannot bind final legacy World data query: "+implementation);
            int index=queries.size();queries.add(query);declarations.remove(signature(query));
            appendQuery(source,query,index);
        }
        for(Method method:declarations.values())if(Modifier.isAbstract(method.getModifiers())){
            source.append("public ").append(typeName(method.getReturnType())).append(' ').append(method.getName()).append('(');parameters(source,method.getParameterTypes());
            source.append(") {throw new UnsupportedOperationException(\"World simulation is not part of oracle data queries\");}\n");
        }
        source.append('}');Files.createDirectories(generated);
        Path java=generated.resolve("NovaLegacyEmptyWorld.java");Files.write(java,source.toString().getBytes(StandardCharsets.UTF_8));
        JavaCompiler compiler=ToolProvider.getSystemJavaCompiler();
        if(compiler==null)throw new IllegalStateException("exact legacy World adapter requires a JDK compiler, not a JRE");
        int status=compiler.run(null,null,null,"-source","8","-target","8","-cp",System.getProperty("java.class.path"),"-d",generated.toString(),java.toString());
        if(status!=0)throw new IOException("cannot compile target-typed legacy World data adapter: "+status);
        Class<?> adapter=Class.forName("NovaLegacyEmptyWorld",true,new URLClassLoader(new URL[]{generated.toUri().toURL()},LegacyWorldAccess.class.getClassLoader()));
        Class<?> unsafeClass=Class.forName("sun.misc.Unsafe");Field field=unsafeClass.getDeclaredField("theUnsafe");field.setAccessible(true);
        Object instance=unsafeClass.getMethod("allocateInstance",Class.class).invoke(field.get(null),adapter);
        BINDINGS.put(instance,new Binding(handler,queries));return instance;
    }

    private static void appendQuery(StringBuilder source,Method method,int index){
        Class<?> result=method.getReturnType();source.append("public ").append(typeName(result)).append(' ').append(method.getName()).append('(');parameters(source,method.getParameterTypes());
        source.append("){try{");if(result!=void.class)source.append("return ").append(result.isPrimitive()?"(("+boxed(result)+")":"("+typeName(result)+")");
        source.append("net.litematicaba.mc26oracle.LegacyWorldAccess.query(this,").append(index).append(",new Object[]{");
        for(int i=0;i<method.getParameterTypes().length;i++){if(i>0)source.append(',');source.append('p').append(i);}source.append("})");
        if(result.isPrimitive()&&result!=void.class)source.append(").").append(result.getName()).append("Value()");
        source.append(";");if(result==void.class)source.append("return;");source.append("}catch(RuntimeException e){throw e;}catch(Error e){throw e;}catch(Throwable e){throw new IllegalStateException(e);}}\n");
    }
    private static void parameters(StringBuilder source,Class<?>[]types){for(int i=0;i<types.length;i++){if(i>0)source.append(',');source.append(typeName(types[i])).append(" p").append(i);}}
    private static String signature(Method method){StringBuilder key=new StringBuilder(method.getName());for(Class<?>parameter:method.getParameterTypes())key.append('/').append(parameter.getName());return key.toString();}
    private static String typeName(Class<?> type){return type.isArray()?typeName(type.getComponentType())+"[]":type.getName().replace('$','.');}
    private static String boxed(Class<?>type){if(type==boolean.class)return"Boolean";if(type==byte.class)return"Byte";if(type==short.class)return"Short";if(type==int.class)return"Integer";if(type==long.class)return"Long";if(type==float.class)return"Float";if(type==double.class)return"Double";if(type==char.class)return"Character";throw new IllegalArgumentException("unsupported primitive "+type);}
}
