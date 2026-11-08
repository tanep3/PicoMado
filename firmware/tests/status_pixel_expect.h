#pragma once
/* Independent bitmap oracle: fixed legacy columns and explicit digit2 rows.
 * Does not include or call the production font lookup. */
static const uint8_t *expected_glyph(char ch){
    static const uint8_t h[]={0x7f,8,8,8,0x7f},o[]={0x3e,0x41,0x41,0x41,0x3e};
    static const uint8_t m[]={0x7f,2,12,2,0x7f},e[]={0x7f,0x49,0x49,0x49,0x41};
    static const uint8_t n[]={0x7f,4,8,16,0x7f},t[]={1,1,0x7f,1,1};
    static const uint8_t w[]={0x7f,0x20,0x18,0x20,0x7f},r[]={0x7f,9,0x19,0x29,0x46};
    static const uint8_t k[]={0x7f,8,0x14,0x22,0x41},a[]={0x7e,0x11,0x11,0x11,0x7e};
    static const uint8_t y[]={3,4,0x78,4,3};
    switch(ch){case 'H':return h;case 'O':return o;case 'M':return m;case 'E':return e;
    case 'N':return n;case 'T':return t;case 'W':return w;case 'R':return r;case 'K':return k;
    case 'A':return a;case 'Y':return y;default:return NULL;}
}
static bool expected_ink(char ch,unsigned col,unsigned row){
    if(ch=='2'){
#ifdef PM_TEST_LEGACY_DIGITS
        (void)col;(void)row;return false;
#else
        const char *rows[]={".###.","#...#","....#","...#.","..#..",".#...","#####"};
        return rows[row][col]=='#';
#endif
    }
    const uint8_t *glyph=expected_glyph(ch);return glyph&&(glyph[col]&(1u<<row));
}
static void expected_line(uint16_t *pixels,const char *s,unsigned y,unsigned limit,uint16_t color){
    size_t n=strlen(s);if(n>limit)n=limit;unsigned left=(PM_W-(unsigned)n*12)/2;
    for(unsigned i=0;i<n;i++)for(unsigned row=0;row<14;row++)for(unsigned col=0;col<10;col++)
        if(expected_ink(s[i],col/2,row/2))pixels[(y+row)*PM_W+left+i*12+col]=color;
}
static void expect_status(const uint16_t *actual,const char *two,unsigned y1,unsigned y2,unsigned limit,uint16_t bg,uint16_t fg,uint16_t second){
    static uint16_t expected[PM_W*PM_H];for(unsigned i=0;i<PM_W*PM_H;i++)expected[i]=bg;
    expected_line(expected,"NETWORK",y1,limit,fg);expected_line(expected,two,y2,limit,second);
    assert(!memcmp(expected,actual,sizeof expected));
}
static void save_ppm(const char *path,const uint16_t *pixels){
    FILE *f=fopen(path,"wb");assert(f);fprintf(f,"P6\n%u %u\n255\n",PM_W,PM_H);
    for(unsigned i=0;i<PM_W*PM_H;i++){
        uint16_t c=pixels[i];unsigned char rgb[]={(unsigned char)(((c>>11)&31)*255/31),(unsigned char)(((c>>5)&63)*255/63),(unsigned char)((c&31)*255/31)};
        assert(fwrite(rgb,1,3,f)==3);
    }
    assert(!fclose(f));
}
