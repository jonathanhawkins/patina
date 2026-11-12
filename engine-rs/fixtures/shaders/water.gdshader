shader_type spatial;
render_mode cull_back, diffuse_burley, specular_schlick_ggx;

uniform vec4 water_color : source_color = vec4(0.1, 0.4, 0.6, 1.0);
uniform float wave_speed = 0.5;
uniform float wave_height : hint_range(0.0, 2.0) = 0.25;
uniform sampler2D noise_texture;

void vertex() {
	float wave = sin(VERTEX.x * 2.0 + TIME * wave_speed) * wave_height;
	VERTEX.y += wave;
}

void fragment() {
	vec3 noise = texture(noise_texture, UV + TIME * 0.02).rgb;
	ALBEDO = water_color.rgb + noise * 0.05;
	METALLIC = 0.2;
	ROUGHNESS = 0.1;
}
