//! Donde abrir la ventana: la posicion guardada si sigue cayendo en algun monitor actual; si no
//! (monitor desconectado, resolucion distinta), la de fabrica arriba a la derecha. Logica pura,
//! en px fisicos de pantalla; la ventana solo le pasa los rectangulos de los monitores.

/// Rectangulo `[left, top, right, bottom]` en px fisicos.
pub type Rect = [i32; 4];

/// Cuanto de la ventana tiene que quedar dentro de un monitor (px fisicos por lado) para darla
/// por visible: lo bastante para verla y arrastrarla de vuelta.
pub const MIN_VISIBLE: i32 = 40;

/// Esquina de fabrica: arriba a la derecha del monitor `primary`, `margin` desde el borde derecho
/// y `top` desde arriba (ya en px fisicos).
pub fn default_pos(primary: Rect, width: i32, margin: i32, top: i32) -> [i32; 2] {
    [primary[2] - width - margin, primary[1] + top]
}

/// Interseccion de dos rectangulos (ancho, alto); 0 si no se tocan.
fn overlap(a: Rect, b: Rect) -> (i32, i32) {
    let w = a[2].min(b[2]) - a[0].max(b[0]);
    let h = a[3].min(b[3]) - a[1].max(b[1]);
    (w.max(0), h.max(0))
}

/// La posicion guardada si una ventana de `size` ahi queda visible en algun monitor; si no, la
/// de fabrica.
pub fn choose(
    saved: Option<[i32; 2]>,
    size: (i32, i32),
    monitors: &[Rect],
    fallback: [i32; 2],
) -> [i32; 2] {
    let Some([x, y]) = saved else {
        return fallback;
    };
    let win = [x, y, x.saturating_add(size.0), y.saturating_add(size.1)];
    let need_w = MIN_VISIBLE.min(size.0);
    let need_h = 20.min(size.1);
    let visible = monitors.iter().any(|m| {
        let (w, h) = overlap(win, *m);
        w >= need_w && h >= need_h
    });
    if visible { [x, y] } else { fallback }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: Rect = [0, 0, 1920, 1080];
    const LEFT: Rect = [-1280, 0, 0, 1024];
    const SIZE: (i32, i32) = (285, 100);

    #[test]
    fn fabrica_arriba_a_la_derecha() {
        assert_eq!(default_pos(MAIN, 285, 24, 40), [1611, 40]);
        assert_eq!(default_pos([0, 0, 2880, 1620], 428, 36, 60), [2416, 60]);
    }

    #[test]
    fn sin_guardada_usa_la_de_fabrica() {
        assert_eq!(choose(None, SIZE, &[MAIN], [1611, 40]), [1611, 40]);
    }

    #[test]
    fn guardada_visible_se_respeta() {
        assert_eq!(
            choose(Some([300, 500]), SIZE, &[MAIN], [1611, 40]),
            [300, 500]
        );
        // En el monitor de la izquierda (coordenadas negativas).
        assert_eq!(
            choose(Some([-1000, 200]), SIZE, &[MAIN, LEFT], [1611, 40]),
            [-1000, 200]
        );
        // Medio fuera por la derecha pero con bastante dentro para agarrarla.
        assert_eq!(
            choose(Some([1800, 10]), SIZE, &[MAIN], [1611, 40]),
            [1800, 10]
        );
    }

    #[test]
    fn monitor_desconectado_vuelve_a_la_de_fabrica() {
        // Estaba en el monitor de la izquierda, que ya no esta.
        assert_eq!(
            choose(Some([-1000, 200]), SIZE, &[MAIN], [1611, 40]),
            [1611, 40]
        );
        // Casi toda fuera: solo 10 px dentro.
        assert_eq!(
            choose(Some([1910, 10]), SIZE, &[MAIN], [1611, 40]),
            [1611, 40]
        );
        // Por debajo de la pantalla.
        assert_eq!(
            choose(Some([300, 1075]), SIZE, &[MAIN], [1611, 40]),
            [1611, 40]
        );
        // Sin monitores (no deberia pasar): la de fabrica.
        assert_eq!(choose(Some([300, 500]), SIZE, &[], [1611, 40]), [1611, 40]);
    }

    #[test]
    fn valores_extremos_no_desbordan() {
        assert_eq!(
            choose(Some([i32::MAX, i32::MAX]), SIZE, &[MAIN], [1611, 40]),
            [1611, 40]
        );
    }
}
